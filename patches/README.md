# Patches for vendored submodules

`vendor/jian` points at the upstream repository, so changes OpenPencil needs
there are kept here until they are upstreamed or the submodule is moved to a fork.

## jian-variable-font-weights.patch

Apply from `vendor/jian`:

    git -C vendor/jian apply ../../patches/jian-variable-font-weights.patch

Registers one clone per CSS weight (100..900, inside the `wght` axis range) for
user-imported variable fonts, so a bold request resolves to a real bold instance
instead of the regular default plus synthetic bold. Without it the weight tests
in `op-host-native` (variable font serves the requested weight) fail and
downloaded variable Google Fonts render at weight 400.
