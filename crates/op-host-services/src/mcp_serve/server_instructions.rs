//! The `instructions` string the MCP `initialize` reply carries.
//!
//! It is what an agent reads once, before it has seen a single tool, so it
//! covers only the rules that are easy to get wrong from the tool list alone:
//! how to find tools, which document a call is about, the read-write-verify
//! loop, units, and how errors read. Per-tool detail stays in the tool
//! descriptions.
//!
//! One text serves every session. `initialize` is answered before a client
//! picks a catalog or a document, so the text is worded to stay true for both
//! the full and the lean catalog (it says "when list_documents is listed",
//! not "call list_documents").
//!
//! Claude Code truncates server instructions at 4,096 characters, so
//! `SERVER_INSTRUCTIONS` must stay under `MAX_INSTRUCTIONS_CHARS`; a test
//! pins that.

/// Characters of headroom under Claude Code's 4,096-character cut.
#[cfg(test)]
pub const MAX_INSTRUCTIONS_CHARS: usize = 4000;

pub const SERVER_INSTRUCTIONS: &str = "\
OpenPencil is a vector design editor. These tools read and edit its documents (.op files).

Finding tools
- Your client may load tool definitions on demand: search by keyword and load a tool before you call it. The lean catalog has six tools (get_editor_state, get_guidelines, batch_design, snapshot_layout, get_screenshot, finalize_design). The full catalog also has its own ToolSearch tool that searches this server's tools.

Which document
- The editor can have several documents open at once. When list_documents is listed, call it first and pass the documentId it returns (d<N>.<nonce>) on every call. Ids from an earlier editor session are refused.
- If you connected to a URL ending in /mcp/d/<id>, every call is already bound to that document; do not pass documentId.
- Never use filePath on a file that is open in an editor tab: the call is refused. Edit an open document through its documentId. filePath is only for a file on disk that no tab holds, inside the project folder.

Working loop
1. Read before you write. Use get_editor_state (lean) or read_nodes / snapshot_layout (full) to see the current page. Node ids are strings: copy them from results, never guess them.
2. Once per task, read get_guidelines for the kind of screen you are building.
3. Write. batch_design builds new UI or applies many changes at once (script, operations or nodes_json; it is transactional, so a failing line applies nothing and errors[] lists every failure). Keep each call to about 25 operations and build large screens section by section. Use the small writers (set_node_*, update_node, move_node, delete_node) for one-off edits. Design writer results may carry layoutIssues, iconIssues, contrastIssues and similar keys: fix them before finalize_design. snapshot_layout entries may carry clipped or collapsed. When design_skeleton, design_content and design_refine are listed (full catalog), they are the layered way to build a whole new page: skeleton once, content once per section, then refine. With the lean catalog they are absent, so build with batch_design script.
4. Verify. snapshot_layout returns the bounds tree (numbers, not pixels) so you can check sizes and positions cheaply; get_screenshot shows how it looks. In the full catalog, lint_document and get_design_quality report concrete problems (overflow, contrast, structure) and are cheaper than a screenshot.
5. Finish. After you build or change a design with batch_design or the design_* tools, call finalize_design last, once, and fix every advisory it reports before you tell the user the design is done.
6. Save. When save_document is listed, call it without filePath as your last step: it saves the document to its own file (an untitled document needs create_document first). The editor may also save when your run ends.

Units
- x, y, width, height and corner radius are document pixels. They do not depend on the editor's zoom or on a screenshot's scale.

Fonts
- A font that is not installed falls back to a default, and the editor downloads Google Fonts families that are missing when that setting is on, so prefer widely available families and do not invent family names.

Errors and retries
- A failed call has isError true and text of the form `Error: [Code] message`. InvalidArgument and MissingArgument name the argument to fix. An unknown node id says so: get valid ids from read_nodes or find_node_by_name. A request the server cannot parse at all (broken JSON, an array or object where a scalar is required) comes back as a JSON-RPC error whose message names the tool and the argument, not as `[Code]` text.
- After a failed call, do not guess: re-read the state (get_editor_state in the lean catalog, read_nodes or snapshot_layout in the full one; list_documents if the document itself is in doubt), fix the cause the error names, then retry.
- On the live editor a write may carry an optional operationId. Retrying a timed-out write with the same operationId and arguments never applies it twice.
";
