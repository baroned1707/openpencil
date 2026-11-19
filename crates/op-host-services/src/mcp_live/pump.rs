//! The UI-thread half of the live server: drain the connection threads'
//! requests and answer each from the document it names.
//!
//! The server holds no `EditorState`. Every pump is handed a
//! [`DocumentResolver`] by the host, and each request carries the id of the
//! document it is about, so a snapshot and the apply that follows are both
//! resolved by id and the apply is checked against THAT document's epoch.

use super::*;

/// See [`McpLiveServer::pump`].
const STALE_REQUEST_SCAN_FACTOR: usize = 8;

impl McpLiveServer {
    /// Answer up to [`UI_PUMP_REQUEST_BUDGET`] queued requests. Call it from
    /// the UI thread whenever the server wakes it, and on redraw.
    ///
    /// A request whose caller has stopped waiting is dropped without running
    /// (see `request_ticket`). Dropping costs nothing, so it does not use up
    /// the answer budget, but at most [`STALE_REQUEST_SCAN_FACTOR`] times the
    /// budget is dequeued per call.
    pub fn pump(&mut self, resolver: &mut dyn DocumentResolver) -> McpPumpOutcome {
        let mut outcome = McpPumpOutcome::default();
        let mut answered = 0;
        for _ in 0..UI_PUMP_REQUEST_BUDGET * STALE_REQUEST_SCAN_FACTOR {
            if answered == UI_PUMP_REQUEST_BUDGET {
                break;
            }
            let request = match self.req_rx.try_recv() {
                Ok(request) => request,
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => break,
            };
            // First thing, before any resolver or filesystem work: a request
            // nobody is waiting for is dropped unrun.
            let ticket = request.ticket().cloned();
            if let Some(ticket) = &ticket {
                if !self.admit(ticket, request.kind()) {
                    continue;
                }
            }
            answered += 1;
            let mut stop_after = false;
            match request {
                UiRequest::Documents { ack } => {
                    let _ = ack.send(resolver.documents());
                }
                UiRequest::Snapshot { document, ack, .. } => {
                    let mut taken = None;
                    let resolved = resolver.with_document(document, &mut |state, host_epoch| {
                        // Narrowed clone: the MCP registry + applier never
                        // read `chat` / `codegen` / `theme_presets`, and those
                        // are the sub-states that grow with SESSION length
                        // (transcripts, generated source, saved preset
                        // tables) rather than with the document — so copying
                        // them into every tool call's snapshot is pure
                        // UI-thread stall. See
                        // `op_editor_core::request_snapshot` for the field
                        // audit and for why an `Arc` can't replace this clone
                        // (the connection thread re-applies the ack'd command
                        // to its own copy, and the live state keeps mutating
                        // here).
                        taken = Some(SnapshotAck {
                            state: op_editor_core::request_snapshot::narrowed_snapshot(state),
                            epoch: DocumentEpoch::of(host_epoch, state),
                        });
                    });
                    let _ =
                        ack.send(resolved.and_then(|()| taken.ok_or(ResolveError::NotReachable)));
                }
                UiRequest::ListPages { document, ack, .. } => {
                    let mut pages = None;
                    let resolved = resolver.with_document(document, &mut |state, _| {
                        pages = Some(op_mcp::list_pages_snapshot(state));
                    });
                    let _ =
                        ack.send(resolved.and_then(|()| pages.ok_or(ResolveError::NotReachable)));
                }
                UiRequest::Apply {
                    document,
                    tool_name,
                    cmd,
                    expected_epoch,
                    ack,
                    ..
                } => {
                    let mut applied = ApplyAck::default();
                    let resolved = resolver.with_document(document, &mut |state, host_epoch| {
                        applied = self.apply_command(
                            state,
                            host_epoch,
                            &tool_name,
                            &cmd,
                            expected_epoch,
                            &mut outcome,
                        );
                    });
                    applied.unresolved = resolved.err();
                    let _ = ack.send(applied);
                }
                UiRequest::ReplaceDocument {
                    doc,
                    editor_meta,
                    ack,
                    ..
                } => {
                    // Whole-document sync (a REST route with no document id)
                    // always means the document on screen.
                    let mut doc = Some(doc);
                    let mut editor_meta = Some(editor_meta);
                    let mut accepted = false;
                    let resolved = self.with_active(resolver, &mut |state, _| {
                        if let Err(reason) = CollabGatePolicy::from(&state.editor_ui.collab)
                            .check(CollabGateAction::ReplaceDocument, CollabEditSource::Mcp)
                        {
                            state.editor_ui.collab.set_notice(
                                reason.notice_kind(),
                                crate::design_agent_tools::reveal_now_millis(),
                            );
                            outcome.repaint = true;
                            return;
                        }
                        // The document was already loaded off the UI thread;
                        // just swap it in (preserving editor chrome). Layout
                        // is computed by the renderer at paint, so the swap +
                        // dirty flag repaints the new document with no extra
                        // layout pass here.
                        if let (Some(doc), Some(meta)) = (doc.take(), editor_meta.take()) {
                            state.replace_document(*doc);
                            op_pen_loader::apply_editor_meta(state, meta);
                            accepted = true;
                            outcome.repaint = true;
                            outcome.layout_dirty = true;
                        }
                    });
                    if let (Ok(target), true) = (&resolved, accepted) {
                        outcome.replaced.push(*target);
                    }
                    let _ = ack.send(resolved.map(|_| accepted));
                }
                UiRequest::UpdateEditorMeta {
                    editor_meta, ack, ..
                } => {
                    let mut editor_meta = Some(editor_meta);
                    let mut accepted = false;
                    let resolved = self.with_active(resolver, &mut |state, _| {
                        // Root metadata is part of the shared document while a
                        // session is bound: an MCP write must not change it.
                        if let Err(reason) = CollabGatePolicy::from(&state.editor_ui.collab).check(
                            CollabGateAction::Document(CollabDocumentMutation::Unsupported(
                                CollabUnsupportedFeature::RootMetadata,
                            )),
                            CollabEditSource::Mcp,
                        ) {
                            state.editor_ui.collab.set_notice(
                                reason.notice_kind(),
                                crate::design_agent_tools::reveal_now_millis(),
                            );
                            outcome.repaint = true;
                            return;
                        }
                        if let Some(meta) = editor_meta.take() {
                            op_pen_loader::apply_editor_meta(state, meta);
                        }
                        accepted = true;
                        outcome.repaint = true;
                        outcome.layout_dirty = true;
                    });
                    let _ = ack.send(resolved.map(|_| accepted));
                }
                UiRequest::GenerateDesignMd { request } => {
                    self.pending_design_md.push_back(request);
                }
                UiRequest::CreateDocument {
                    document,
                    project_root,
                    name,
                    expected_epoch,
                    ack,
                    ..
                } => {
                    let listings = resolver.documents();
                    let bound_path = listings
                        .iter()
                        .find(|listing| listing.id == document)
                        .and_then(|listing| listing.path.clone());
                    let paths_open_elsewhere: Vec<_> = listings
                        .iter()
                        .filter(|listing| listing.id != document)
                        .filter_map(|listing| listing.path.clone())
                        .collect();
                    let mut answer = None;
                    let mut created_here = false;
                    let resolved = resolver.with_document(document, &mut |state, host_epoch| {
                        if expected_epoch != DocumentEpoch::of(host_epoch, state) {
                            answer = Some(CreateAck::DocumentChanged);
                            return;
                        }
                        let result = create_document_route::create_on_ui_thread(
                            state,
                            bound_path.as_deref(),
                            &paths_open_elsewhere,
                            &project_root,
                            &name,
                        );
                        if let Ok(created) = &result {
                            outcome.created.push((document, created.file_path.clone()));
                            created_here = true;
                        }
                        answer = Some(CreateAck::Done(result));
                        outcome.repaint = true;
                    });
                    let _ = ack.send(match (resolved, answer) {
                        (Err(error), _) => CreateAck::Unresolved(error),
                        (Ok(()), Some(answer)) => answer,
                        (Ok(()), None) => CreateAck::DocumentChanged,
                    });
                    if created_here {
                        // The host binds the new file to its document right
                        // after this pump returns. A request queued behind
                        // this one must see the bound path, so stop here.
                        stop_after = true;
                    }
                }
                UiRequest::SaveInPlace {
                    document,
                    claimed_path,
                    ack,
                    ..
                } => {
                    let target = resolver.in_place_save_target(document);
                    // Not this document's file: the call is an ordinary copy,
                    // and nothing here is saved or recorded.
                    if save_in_place_route::is_not_own_path(&target, claimed_path.as_deref()) {
                        let _ = ack.send(SaveAck::NotOwnPath);
                    } else {
                        let mut answer = None;
                        let resolved = resolver.with_document(document, &mut |state, _| {
                            answer = Some(save_in_place_route::save_on_ui_thread(state, &target));
                        });
                        let saved = answer.as_ref().and_then(|result| result.as_ref().ok());
                        if let Some(saved) = saved {
                            outcome.saved_in_place.push((document, saved.clone()));
                            outcome.repaint = true;
                        }
                        // The host records the new on-disk baseline right
                        // after this pump returns. A second save queued behind
                        // this one would compare against the stale one and be
                        // refused as a conflict, so stop here.
                        stop_after = saved.is_some();
                        let _ = ack.send(match (resolved, answer) {
                            (Err(error), _) => SaveAck::Unresolved(error),
                            (Ok(()), Some(result)) => SaveAck::Done(result),
                            (Ok(()), None) => SaveAck::Unresolved(ResolveError::NotReachable),
                        });
                    }
                }
                #[cfg(feature = "mcp-debug-tools")]
                UiRequest::Screenshot {
                    document,
                    spec,
                    ack,
                } => {
                    // Read-only render of the live state — no repaint needed.
                    let mut captured = None;
                    let resolved = resolver.with_document(document, &mut |state, _| {
                        captured = Some(crate::export::screenshot::capture(state, &spec));
                    });
                    let _ = ack
                        .send(resolved.and_then(|()| captured.ok_or(ResolveError::NotReachable)));
                }
            }
            if let Some(ticket) = ticket {
                ticket.finish();
            }
            if stop_after {
                break;
            }
        }
        outcome
    }

    /// Whether the request may run. `false`: its caller has stopped waiting
    /// (the deadline passed, or the caller cancelled), so it is dropped
    /// unrun, counted, and left unanswered. The caller learns it from the
    /// ticket and the closed answer channel; if it is already gone, nothing
    /// is lost.
    fn admit(&mut self, ticket: &Ticket, kind: &str) -> bool {
        if ticket.try_start(self.timing.now()) {
            return true;
        }
        self.expired_requests += 1;
        eprintln!(
            "openpencil-desktop mcp: dropped an expired {kind} request unapplied ({} so far)",
            self.expired_requests
        );
        false
    }

    /// Run `f` on the document on screen and return which document that was,
    /// so a follow-up can be recorded against it.
    fn with_active(
        &self,
        resolver: &mut dyn DocumentResolver,
        f: &mut dyn FnMut(&mut EditorState, u64),
    ) -> Result<DocumentId, ResolveError> {
        let active = resolver.active_document().ok_or(ResolveError::Unknown)?;
        resolver.with_document(active, f)?;
        Ok(active)
    }

    /// Apply one command to the lent document, unless the document it was
    /// derived from is gone or the collaboration session refuses it.
    fn apply_command(
        &mut self,
        state: &mut EditorState,
        host_epoch: u64,
        tool_name: &str,
        cmd: &EditorCommand,
        expected_epoch: Option<DocumentEpoch>,
        outcome: &mut McpPumpOutcome,
    ) -> ApplyAck {
        // The snapshot this command was derived from belongs to a document
        // that has since been replaced: node ids repeat across documents, so
        // applying would silently edit the wrong one.
        if expected_epoch.is_some_and(|epoch| epoch != DocumentEpoch::of(host_epoch, state)) {
            return ApplyAck {
                document_changed: true,
                ..ApplyAck::default()
            };
        }
        if let Err(reason) = CollabGatePolicy::from(&state.editor_ui.collab)
            .check_command(cmd, CollabEditSource::Mcp)
        {
            state.editor_ui.collab.set_notice(
                reason.notice_kind(),
                crate::design_agent_tools::reveal_now_millis(),
            );
            outcome.repaint = true;
            return ApplyAck::default();
        }
        let layout_dirty = command_invalidates_layout(cmd);
        // Snapshot BEFORE apply, only for the one tool the canvas radar-scan
        // cares about — `design_agent_tools.rs`'s
        // `should_register_batch_reveals` gates the in-app design-agent
        // loop's reveal registration the exact same way, for the exact same
        // reason (every other write tool is a targeted property tweak, not a
        // "generation").
        let ids_before = (tool_name == "batch_design")
            .then(|| crate::design_agent_tools::collect_active_node_ids(state));
        let icon_names_before = op_chat_agent::design_agent_tools::IconNameSnapshot::capture(state);
        let applied = state.apply(cmd.clone());
        if applied {
            let write_repairs =
                crate::mcp_serve::run_write_repairs_after_apply(state, &icon_names_before);
            if let Some(ids_before) = ids_before {
                self.register_mcp_generation(&ids_before, state);
            }
            outcome.layout_dirty |= write_repairs;
            outcome.repaint = true;
            outcome.layout_dirty |= layout_dirty;
        }
        ApplyAck {
            applied,
            ..ApplyAck::default()
        }
    }
}
