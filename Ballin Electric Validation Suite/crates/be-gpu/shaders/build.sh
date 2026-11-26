#!/usr/bin/env bash
# Recompile SPIR-V from GLSL (requires glslangValidator from glslang-tools).
# The .spv outputs are committed so normal builds need no shader compiler.
set -euo pipefail
cd "$(dirname "$0")"
for s in kat.comp vram.comp fullscreen.vert fur.frag; do
  glslangValidator -V --target-env vulkan1.0 -o "spv/$s.spv" "$s" >/dev/null
  spirv-val --target-env vulkan1.0 "spv/$s.spv"
done
echo "SPIR-V rebuilt and validated"
