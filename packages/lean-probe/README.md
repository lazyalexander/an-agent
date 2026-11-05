# lean-probe

English · [简体中文](../../doc/zh-CN/packages/lean-probe/README.md)

A tiny Lake package for the Lean 4 compatibility probe. No Mathlib.

The `lean_tool` test copies these files into a temp dir and runs `lake build` through the `Tool` adapter. Do not treat this as a kernel constructor — it is an academic-CLI fixture.
