# lean-probe

[English](../../../../packages/lean-probe/README.md) · 中文译文，以英文为准。 · [中文目录](../../README.md)

一个用于Lean 4兼容性探测的微型Lake包。不包含Mathlib。

`lean_tool`测试会将这些文件复制到一个临时目录中，并通过`Tool`适配器运行`lake build`。请不要将此视为内核构造器——这是一个学术CLI测试装置。
