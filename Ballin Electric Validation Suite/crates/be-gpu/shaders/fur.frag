#version 450
// Original procedural "fur ring" shading workload (FurMark-inspired in
// purpose only: sustained fragment load). Fully deterministic for a given
// push-constant input, so repeated frames can be verified bit-exactly.
layout(location = 0) out vec4 color;
layout(push_constant) uniform PC { float t; uint layers; uint width; uint height; } pc;

float h(vec2 p) { return fract(sin(dot(p, vec2(127.1, 311.7))) * 43758.5453); }
float n(vec2 p) {
    vec2 i = floor(p), f = fract(p);
    vec2 u = f * f * (3.0 - 2.0 * f);
    return mix(mix(h(i), h(i + vec2(1, 0)), u.x), mix(h(i + vec2(0, 1)), h(i + vec2(1, 1)), u.x), u.y);
}
void main() {
    vec2 uv = gl_FragCoord.xy / vec2(pc.width, pc.height) * 2.0 - 1.0;
    float r = length(uv), a = atan(uv.y, uv.x);
    vec3 c = vec3(0.0);
    for (uint i = 0u; i < pc.layers; i++) {
        float fi = float(i) / float(pc.layers);
        vec2 q = vec2(a * 12.0 + fi * 3.0, r * 40.0 - pc.t * (1.0 + fi));
        float strand = smoothstep(0.55, 1.0, n(q) * n(q * 2.3 + fi));
        float ring = exp(-abs(r - 0.55 - 0.15 * sin(a * 5.0 + pc.t)) * 8.0);
        c += strand * ring * vec3(0.55 + 0.45 * fi, 0.60, 0.65 - 0.3 * fi) / float(pc.layers);
    }
    color = vec4(clamp(c * 2.2, 0.0, 1.0), 1.0);
}
