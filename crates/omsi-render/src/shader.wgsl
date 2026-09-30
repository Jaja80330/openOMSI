struct Camera {
    view_proj: mat4x4<f32>,
    cam_pos: vec4<f32>,
    world_origin: vec4<f32>, // world-space origin removed from render coordinates
    sun_dir: vec4<f32>,      // xyz direction to the sun, w intensity
    ambient: vec4<f32>,      // rgb ambient (envir light C)
    fog: vec4<f32>,          // rgb, density
    sun_color: vec4<f32>,    // rgb (envir light A), w night factor 0..1 (nightmap strength)
    sky_color: vec4<f32>,    // rgb secondary light from above (envir light B), w 1 = vanilla (as OMSI 2), 0 = Vanilla+
    light_grid: vec4<f32>,   // x,y grid origin (render-origin relative), z cell size, w cells per side
    sky: vec4<f32>,
    cam_right: vec4<f32>,
    cam_up: vec4<f32>,
    clouds: vec4<f32>,
    light_view_proj: mat4x4<f32>,
    light_view_proj_far: mat4x4<f32>,
    shadow: vec4<f32>,       // x enabled, y texel size, z near cascade half range
    post: vec4<f32>,         // x enhanced, y time, zw sun ndc
    inside_a: vec4<f32>,     // player vehicle box: origin xyz, sin(heading)
    inside_b: vec4<f32>,     // cos(heading), half extents xyz
    inside_c: vec4<f32>,     // box centre offset xyz, w = 1 when there is a box
    flags: vec4<f32>,        // x detail texturing, y enhanced graphics, z never set (see fs_main's end), w close cascade half range
    light_view_proj_close: mat4x4<f32>,
};

// 1 when the point lies inside the player's vehicle (its [boundingbox], shrunk a little so
// that the outer skin, the glass and the roof stay outside): weather stays out of the cab.
// The side/front/back shrink was a bare 0.12 m, which left an interior fixture close to the
// wall (a window sill, the handbrake by the windscreen) just outside the box, wearing the
// up-facing wet/snow coat meant for the body panel beside it (see the `outside` weight
// below). A previous fix WIDENED this margin to 0.6 m, reasoning by analogy with the falling-
// particle exclusion box in `rain.rs` - but that box works the other way round (its margin
// is added, growing the excluded region outward, away from the body) from this one (whose
// margin is subtracted, shrinking the "inside" region inward, toward the body): widening it
// here shrank the cabin's own "inside" zone and threw the *whole* cabin outside it, coating
// the entire interior in snow instead of just the ledge. Narrowed to 0.03 m instead - just
// enough that the true outer skin and glass stay outside, without giving up real interior
// floor space near the wall.
// A normal of no length (a mesh saved with zero normals) is upright instead of NaN:
// normalize(0) is NaN on Vulkan and Metal and the pixel went black.
fn safe_normal(v: vec3<f32>) -> vec3<f32> {
    let d = dot(v, v);
    if (d > 1e-20) {
        return v * inverseSqrt(d);
    }
    return vec3<f32>(0.0, 0.0, 1.0);
}

fn inside_vehicle(world: vec3<f32>) -> f32 {
    if (camera.inside_c.w < 0.5) {
        return 0.0;
    }
    let d = world - camera.inside_a.xyz;
    let sh = camera.inside_a.w;
    let ch = camera.inside_b.x;
    let x = d.x * ch - d.y * sh - camera.inside_c.x;
    let y = d.x * sh + d.y * ch - camera.inside_c.y;
    let z = d.z - camera.inside_c.z;
    let h = camera.inside_b.yzw;
    let in_x = abs(x) < h.x - 0.03;
    let in_y = abs(y) < h.y - 0.03;
    let in_z = z > -h.z - 0.6 && z < h.z - 0.03;
    return select(0.0, 1.0, in_x && in_y && in_z);
}

// 1 when the point belongs to the player's vehicle: its `[boundingbox]` grown a little,
// so that the skin and the glass the cab test (`inside_vehicle`) leaves out are in.
fn near_player_vehicle(world: vec3<f32>) -> f32 {
    if (camera.inside_c.w < 0.5) {
        return 0.0;
    }
    let d = world - camera.inside_a.xyz;
    let sh = camera.inside_a.w;
    let ch = camera.inside_b.x;
    let x = d.x * ch - d.y * sh - camera.inside_c.x;
    let y = d.x * sh + d.y * ch - camera.inside_c.y;
    let z = d.z - camera.inside_c.z;
    let h = camera.inside_b.yzw + vec3<f32>(0.25);
    return select(0.0, 1.0, abs(x) < h.x && abs(y) < h.y && abs(z) < h.z);
}

// How much of the way from the camera to `world` lies outside the player's vehicle: there is
// no fog in the cab. From the driver's seat the whole saloon (seats 2 m away, the doors 5 m
// away) took the fog of that distance and in thick ground fog the interior went milky, the
// door panes nearly opaque - outside the bus there was nothing of the kind. The box is the
// one `inside_vehicle` uses; the part of the ray inside it is taken off the fogged distance.
fn fog_distance(world: vec3<f32>) -> f32 {
    let full = distance(world, camera.cam_pos.xyz);
    if (camera.inside_c.w < 0.5) {
        return full;
    }
    let sh = camera.inside_a.w;
    let ch = camera.inside_b.x;
    let to_local = fn_box_local(camera.cam_pos.xyz);
    let p1 = fn_box_local(world);
    let d = p1 - to_local;
    let h = camera.inside_b.yzw;
    // slab test of the segment against the box (x, y the plan, z height)
    var t0 = 0.0;
    var t1 = 1.0;
    for (var k = 0; k < 3; k = k + 1) {
        let o = to_local[k];
        let dk = d[k];
        let hk = h[k];
        if (abs(dk) < 1e-6) {
            if (abs(o) > hk) {
                return full;
            }
        } else {
            let a = (-hk - o) / dk;
            let b = (hk - o) / dk;
            t0 = max(t0, min(a, b));
            t1 = min(t1, max(a, b));
        }
    }
    if (t1 <= t0) {
        return full;
    }
    return max(full * (1.0 - (t1 - t0)), 0.0);
}

// A point in the player's vehicle box frame (centred, x across, y along, z up).
fn fn_box_local(world: vec3<f32>) -> vec3<f32> {
    let d = world - camera.inside_a.xyz;
    let sh = camera.inside_a.w;
    let ch = camera.inside_b.x;
    return vec3<f32>(d.x * ch - d.y * sh - camera.inside_c.x, d.x * sh + d.y * ch - camera.inside_c.y, d.z - camera.inside_c.z);
}

// 1 where the weather reaches the point. The cab test above reaches 0.6 m below the
// vehicle's box so that the floor stays dry, and that takes in the road under the bus as
// well: the snow on it vanished in a bus-shaped patch of bare asphalt (which read as the
// bus's shadow wiping the snow off). The ground - terrain, roads, painted ground, flagged
// by `terrain` or the instance's surface flag - is never inside a vehicle.
fn weather_outside(world: vec3<f32>, terrain: bool, surface: f32) -> f32 {
    if (terrain || surface > 0.5) {
        return 1.0;
    }
    return 1.0 - inside_vehicle(world);
}

// As `weather_outside`, knowing the surface's normal: the bus's own outer skin is outside
// even where it lies inside the box. A side wall leans in towards the roof (and bulges over
// the wheel arches), so the box's side plane cuts through the panel: above the cut the paint
// took the cab's light and stayed dry, below it the sky's - the wall looked half glossy,
// half matt, along a sharp line. A face within 0.35 m of a side, the front, the back or the
// roof of the box and turned out through it is skin, not cabin.
fn weather_outside_n(world: vec3<f32>, n: vec3<f32>, terrain: bool, surface: f32) -> f32 {
    if (terrain || surface > 0.5) {
        return 1.0;
    }
    // a vehicle's part (lib.rs `Instance::roof`): under its roof it is dry, whichever
    // vehicle it is - the one the camera is in, another player's, a timetable bus
    if (surface < -500.0) {
        let roof = -surface - 5000.0;
        if (world.z < roof - 0.3) {
            return 0.0;
        }
    }
    if (inside_vehicle(world) < 0.5) {
        return 1.0;
    }
    let d = world - camera.inside_a.xyz;
    let sh = camera.inside_a.w;
    let ch = camera.inside_b.x;
    let x = d.x * ch - d.y * sh - camera.inside_c.x;
    let y = d.x * sh + d.y * ch - camera.inside_c.y;
    let z = d.z - camera.inside_c.z;
    let nx = n.x * ch - n.y * sh;
    let ny = n.x * sh + n.y * ch;
    let h = camera.inside_b.yzw;
    let skin_x = h.x - abs(x) < 0.35 && nx * sign(x) > 0.5;
    let skin_y = h.y - abs(y) < 0.35 && ny * sign(y) > 0.5;
    let skin_z = h.z - z < 0.35 && n.z > 0.5;
    return select(0.0, 1.0, skin_x || skin_y || skin_z);
}
@group(0) @binding(0) var<uniform> camera: Camera;
// Whether this pipeline draws alpha-tested materials (the only ones that may `discard`).
// Pipelines of every other kind set it false, and the discard is compiled out: a fragment
// function that can discard turns the GPU's early depth test off - on Apple's GPUs the
// hidden surface removal as well - for everything it draws, and the heavy shading then
// ran for every covered layer of the city, not once per pixel.
override ALPHA_TEST: bool = true;
// Multisampled cutout pipelines can turn filtered alpha directly into sample coverage.
override ALPHA_TO_COVERAGE: bool = false;
@group(0) @binding(5) var t_shadow: texture_depth_2d;
@group(0) @binding(6) var s_shadow: sampler_comparison;
@group(0) @binding(7) var t_shadow_far: texture_depth_2d;
@group(0) @binding(8) var t_ao: texture_2d<f32>;
@group(0) @binding(9) var s_ao: sampler;

// The ambient occlusion at a pixel of the full picture. It is worked out at half size, and
// a plain bilinear lookup blended the occlusion of the ground behind an edge with that of
// the object in front: every wheel, pole and kerb stood in a pale outline on the shaded
// ground under the bus. Of the four half-size texels around the pixel only those at the
// pixel's own depth count (a depth-aware upsample); none of them: the closest in depth.
fn ao_at(frag: vec2<f32>, world: vec3<f32>) -> f32 {
    let size = vec2<i32>(textureDimensions(t_ao));
    let z = (camera.view_proj * vec4<f32>(world, 1.0)).w;
    let f = frag * 0.5 - vec2<f32>(0.5);
    let base = vec2<i32>(floor(f));
    let fr = f - floor(f);
    let tol = 0.04 + 0.015 * z;
    var sum = 0.0;
    var wsum = 0.0;
    var best = 1.0;
    var best_d = 1e9;
    for (var j = 0; j < 2; j = j + 1) {
        for (var i = 0; i < 2; i = i + 1) {
            let c = clamp(base + vec2<i32>(i, j), vec2<i32>(0), size - vec2<i32>(1));
            let s = textureLoad(t_ao, c, 0);
            let bw = select(1.0 - fr.x, fr.x, i == 1) * select(1.0 - fr.y, fr.y, j == 1);
            let dz = abs(s.g - z);
            let w = bw * (1.0 - smoothstep(0.0, tol, dz)) + 1e-4 * bw;
            sum = sum + s.r * w;
            wsum = wsum + w;
            if (dz < best_d) {
                best_d = dz;
                best = s.r;
            }
        }
    }
    // no AO sample at this surface's depth: the surface is not in the depth prepass (a
    // door or panel drawn in front of what the AO saw) - the AO behind it must not show
    if (best_d > tol * 1.5) {
        return 1.0;
    }
    return select(best, sum / wsum, wsum > 0.02);
}
// the tile light maps (`.map.LM.bmp`) of the 5x5 tiles around the camera, north up, and where
// they lie: x, y the south-west corner (render-origin relative), z the side, w 1 when set
@group(0) @binding(18) var t_lmap: texture_2d<f32>;
@group(0) @binding(19) var<uniform> lmap: vec4<f32>;

// The tile light map's light at a world point (black outside the loaded square).
fn light_map_at(p: vec3<f32>) -> vec3<f32> {
    if (lmap.w < 0.5) {
        return vec3<f32>(0.0);
    }
    let uv = vec2<f32>((p.x - lmap.x) / lmap.z, 1.0 - (p.y - lmap.y) / lmap.z);
    let c = textureSampleLevel(t_lmap, s_ao, clamp(uv, vec2<f32>(0.0), vec2<f32>(1.0)), 0.0).rgb;
    let inside = uv.x >= 0.0 && uv.x <= 1.0 && uv.y >= 0.0 && uv.y <= 1.0;
    return select(vec3<f32>(0.0), c, inside);
}

// A material lit at night by the tile light map (`[LightMapMapping]`, the splines): the
// unlit flag carries 0.35.
fn light_map_mapped(m: vec4<f32>) -> bool {
    return m.y > 0.3 && m.y < 0.45;
}
// (the model matrices as their columns, four vec4 each: some phone GPUs (Mali, Adreno) read an
// array of matrices from a storage buffer wrongly, and every mesh came out flat and far away)
@group(0) @binding(1) var<storage, read> models: array<vec4<f32>>;
fn model_matrix(e: u32) -> mat4x4<f32> {
    let k = e * 4u;
    return mat4x4<f32>(models[k], models[k + 1u], models[k + 2u], models[k + 3u]);
}
// x: alpha multiplier, y: visible (0/1), zw: uv offset
@group(0) @binding(2) var<storage, read> inst_params: array<vec4<f32>>;
// the frame's draw list: the per-draw entry of each drawn instance. Draws of the same mesh
// and material are batched, so the instance index points into this list, not at an entry.
@group(0) @binding(10) var<storage, read> draw_list: array<u32>;
struct PointLight {
    pos: vec4<f32>,   // xyz relative to the render origin, w radius (0: an enhanced-only light)
    color: vec4<f32>, // rgb, w intensity
    dir: vec4<f32>,   // spot direction, w cosine of the outer cone (< -1.5: a point light)
    extra: vec4<f32>, // enhanced path: cosine of the inner cone, core radius, beam gain, radius
};
@group(0) @binding(3) var<storage, read> lights: array<PointLight>;
// per cell CELL_CAP light indices, 0xffffffff = empty
@group(0) @binding(4) var<storage, read> grid: array<u32>;
const CELL_CAP: u32 = 32u;

@group(1) @binding(0) var t_diffuse: texture_2d<f32>;
@group(1) @binding(1) var s_diffuse: sampler;
struct MaterialParams {
    color: vec4<f32>,
    // x: alpha mode (0 opaque, 1 test, 2 blend), y: unlit flag, z: has transmap, w: transmap uses alpha
    params: vec4<f32>,
    // x: terrain material (uv in tile space), y: detail texture repeats per tile (0 = none),
    // z: ground texture repeats per tile, w: has nightmap
    extra: vec4<f32>,
    // x: has lightmap, y: envmap factor, z: moisture, w: has [matl_envmap_mask]
    params2: vec4<f32>,
    // emissive colour (the o3d material's or a [matl_allcolor]'s)
    emissive: vec4<f32>,
    // specular colour and power of the D3D material (power in w; 0 = no highlight)
    specular: vec4<f32>,
    // x: [matl_bumpmap] factor, y: has a bump map, z/w: noZwrite/noZcheck
    bump: vec4<f32>,
    // the PBR set beside the diffuse texture: x normal map, y occlusion, z roughness,
    // w metalness (1 = the map has it; see t_pbr_normal / t_pbr_orm)
    pbr: vec4<f32>,
    // x: one of the bus's own screens (the enhanced glow and FXAA leave it alone)
    flags: vec4<f32>,
};
@group(1) @binding(2) var<uniform> material: MaterialParams;
@group(1) @binding(3) var t_trans: texture_2d<f32>;
@group(1) @binding(4) var t_night: texture_2d<f32>;
@group(1) @binding(5) var t_light: texture_2d<f32>;
@group(1) @binding(6) var t_env: texture_2d<f32>;
@group(1) @binding(7) var t_envmask: texture_2d<f32>;
@group(1) @binding(8) var t_bump: texture_2d<f32>;
@group(1) @binding(9) var t_pbr_normal: texture_2d<f32>;
@group(1) @binding(10) var t_pbr_orm: texture_2d<f32>;

// The reflection mask of a [matl_envmap] material: the alpha of its [matl_envmap_mask]
// texture when it has one, else the diffuse texture's alpha - which reads 1 for a texture
// without an alpha channel (a 24-bit bitmap, DXT1, a JPEG), so its factor alone decides.
fn has_env_mask() -> bool {
    return (u32(material.params2.w + 0.5) & 1u) != 0u;
}

// [matl_transmap] was given (its file there or not: see MaterialExtra::transmap_declared)
fn has_transmap_declared() -> bool {
    return (u32(material.params2.w + 0.5) & 2u) != 0u;
}

fn reflection_mask(uv: vec2<f32>, diffuse_a: f32) -> f32 {
    let mask = textureSample(t_envmask, s_diffuse, uv).a;
    return select(diffuse_a, mask, has_env_mask());
}

// The D3DCOLOR Omsi.exe packs for its environment stage (0x7ff8ed): each channel the
// [matl_envmap] factor x 255 x its light (at most 1), truncated, and OR-ed together shifted
// into place without saturation - a factor over 1 spills into the neighbouring channel
// exactly as there (a factor of 10 at full light reads almost white).
fn omsi_texture_factor(factor: f32, light: vec3<f32>) -> vec3<f32> {
    let m = min(max(light, vec3<f32>(0.0)), vec3<f32>(1.0)) * max(factor, 0.0) * 255.0;
    let r = u32(m.r);
    let g = u32(m.g);
    let b = u32(m.b);
    let packed = (0xffu << 24u) | (r << 16u) | (g << 8u) | b;
    return vec3<f32>(f32((packed >> 16u) & 0xffu), f32((packed >> 8u) & 0xffu), f32(packed & 0xffu)) / 255.0;
}

// [matl_bumpmap]: Direct3D's bump-mapped environment stage moves the sphere-map lookup by
// a du/dv map times the factor. Every bump map in the stock and mod content is a grey
// height map, so the du/dv map has to be its slope; it is taken here as the forward
// differences of the height, a byte of height difference counting as a signed-byte step
// (1/127, hence the 2). The stock bodies' noise (factor 0.05-0.1) makes their reflections a
// little wavy, and the panel lines of the O530 Facelift's map (factor 1) break its
// reflection at every seam.
fn bump_offset(uv: vec2<f32>) -> vec2<f32> {
    let texel = 1.0 / vec2<f32>(textureDimensions(t_bump));
    let h = textureSample(t_bump, s_diffuse, uv).a;
    let hx = textureSample(t_bump, s_diffuse, uv + vec2<f32>(texel.x, 0.0)).a;
    let hy = textureSample(t_bump, s_diffuse, uv + vec2<f32>(0.0, texel.y)).a;
    return clamp(vec2<f32>(h - hx, h - hy) * 2.0, vec2<f32>(-1.0), vec2<f32>(1.0)) * material.bump.x;
}

struct VsIn {
    @location(0) pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @builtin(instance_index) inst: u32,
};
struct VsOut {
    // invariant: the depth prepass and the main pass must compute the same depth to the
    // last bit, the main pass tests against the depth the prepass left
    @builtin(position) @invariant clip: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) params: vec4<f32>,
    @location(4) params2: vec4<f32>,
};

@vertex
fn vs_main(in: VsIn) -> VsOut {
    let e = draw_list[in.inst];
    let m = model_matrix(e);
    let wp = m * vec4<f32>(in.pos, 1.0);
    var out: VsOut;
    // Road surfaces (splines, crossings, markings, a vehicle's shadow blob) are pulled
    // towards the eye along the line of sight - the picture does not move, only the depth -
    // by two centimetres near and more far away (seen from above at a slant that is a few
    // millimetres of height: a tyre on the road does not sink into it). A road a little under the terrain
    // otherwise lost to it: the ground's triangles came through the carriageway in teeth
    // and flickered. A depth bias cannot do it with a floating-point depth buffer: its
    // steps are relative to the depth, well under a millimetre at a hundred metres. The
    // painted ground layers (0.75) are the ground and stay where they are.
    let surf = inst_params[e * 2u + 1u].w;
    var cp = wp.xyz;
    if (surf > 0.9) {
        let to = wp.xyz - camera.cam_pos.xyz;
        let d = length(to);
        // (surface objects, 1.25, a little more than the splines under them)
        let decal = select(0.0, 0.01 + 0.001 * d, surf > 1.1 && surf < 1.5);
        let pull = min(0.02 + 0.002 * d + decal, d * 0.3);
        cp = wp.xyz - to / max(d, 1e-3) * pull;
    }
    out.clip = camera.view_proj * vec4<f32>(cp, 1.0);
    out.world = wp.xyz;
    out.normal = safe_normal((m * vec4<f32>(in.normal, 0.0)).xyz);
    let pr = inst_params[e * 2u];
    out.uv = in.uv + pr.zw;
    out.params = pr;
    out.params2 = inst_params[e * 2u + 1u];
    if (pr.y < 0.5) {
        // invisible: collapse the triangle
        out.clip = vec4<f32>(0.0, 0.0, 2.0, 1.0);
    }
    return out;
}

// Shadow map passes: depth from the sun, alpha-tested materials cut out by their texture.
// A surface that casts - a spline standing clear of the ground, a bridge deck - casts from
// half a metre further away from the sun: what lies right under it (the embankment object
// under a railway, the ground a ramp touches down on) is no darker for it, while the ground
// metres below a deck gets its shadow.
fn shadow_caster_pos(e: u32, wp: vec4<f32>) -> vec4<f32> {
    if (abs(inst_params[e * 2u + 1u].w - 1.0) < 0.01) {
        return vec4<f32>(wp.xyz - camera.sun_dir.xyz * 0.5, wp.w);
    }
    return wp;
}

@vertex
fn vs_shadow(in: VsIn) -> VsOut {
    let e = draw_list[in.inst];
    let m = model_matrix(e);
    let wp = shadow_caster_pos(e, m * vec4<f32>(in.pos, 1.0));
    var out: VsOut;
    out.clip = camera.light_view_proj * wp;
    out.world = wp.xyz;
    out.normal = in.normal;
    let pr = inst_params[e * 2u];
    out.uv = in.uv + pr.zw;
    out.params = pr;
    out.params2 = inst_params[e * 2u + 1u];
    if (pr.y < 0.5) {
        out.clip = vec4<f32>(0.0, 0.0, 2.0, 1.0);
    }
    return out;
}

@vertex
fn vs_shadow_close(in: VsIn) -> VsOut {
    let e = draw_list[in.inst];
    let m = model_matrix(e);
    let wp = shadow_caster_pos(e, m * vec4<f32>(in.pos, 1.0));
    var out: VsOut;
    out.clip = camera.light_view_proj_close * wp;
    out.world = wp.xyz;
    out.normal = in.normal;
    let pr = inst_params[e * 2u];
    out.uv = in.uv + pr.zw;
    out.params = pr;
    out.params2 = inst_params[e * 2u + 1u];
    if (pr.y < 0.5) {
        out.clip = vec4<f32>(0.0, 0.0, 2.0, 1.0);
    }
    return out;
}

@vertex
fn vs_shadow_far(in: VsIn) -> VsOut {
    let e = draw_list[in.inst];
    let m = model_matrix(e);
    let wp = shadow_caster_pos(e, m * vec4<f32>(in.pos, 1.0));
    var out: VsOut;
    out.clip = camera.light_view_proj_far * wp;
    out.world = wp.xyz;
    out.normal = in.normal;
    let pr = inst_params[e * 2u];
    out.uv = in.uv + pr.zw;
    out.params = pr;
    out.params2 = inst_params[e * 2u + 1u];
    if (pr.y < 0.5) {
        out.clip = vec4<f32>(0.0, 0.0, 2.0, 1.0);
    }
    return out;
}

@fragment
fn fs_shadow(in: VsOut) {
}

@fragment
fn fs_shadow_test(in: VsOut) {
    var duv = in.uv;
    if (material.extra.x > 0.5) {
        duv = in.uv * material.extra.z;
    }
    // Use the light-pass footprint for cutout coverage. Forcing mip 0 here aliases dense
    // foliage and alpha layers into a checkerboard when the light projection moves by a
    // texel; the mip-aware sample keeps the caster coverage coherent with its resolution.
    // For a material promoted to the shadow pass from Blend, diffuse alpha is commonly a
    // paint/gloss value rather than coverage (vehicle bodies use values such as 0.03). Only
    // alpha-test materials use diffuse alpha as a cutout; blended bodies are solid unless a
    // real transmap supplies coverage.
    var a = select(textureSample(t_diffuse, s_diffuse, duv).a, 1.0, material.params.x > 1.5 && material.params.z < 0.5);
    if (material.params.z > 0.5) {
        let tm = textureSample(t_trans, s_diffuse, in.uv);
        a = select(1.0, tm.a, material.params.w > 0.5);
    }
    if (a < 0.5) {
        discard;
    }
}

// A vehicle body and its windows are often one mesh/material. OMSI represents that
// material as alpha-blended because the transmap contains the window opacity, but the
// alpha-tested body pixels still have to occlude vehicles behind them. The normal depth prepass
// deliberately skips all blended materials, so those pixels would otherwise have no depth
// until the colour pass (where blend order can expose the bus through the car). Keep only
// the effectively opaque part of a transmap in a separate depth-only pass; window pixels
// remain out of the prepass and are composited normally.
@fragment
fn fs_transmap_depth(in: VsOut) {
    if (material.params.z < 0.5) {
        discard;
    }
    let tm = textureSample(t_trans, s_diffuse, in.uv);
    let a = select(1.0, tm.a, material.params.w > 0.5) * in.params.x;
    // Only what the colour pass will cover completely may hide what lies behind it: a
    // texel that is merely more opaque than not (the dimmer and anti-aliased dots of a
    // display's text layer, whose transmap is its script texture) wrote depth here, the
    // display's backplate behind it was then rejected, and the half-transparent text was
    // blended over the sky - holes in the display.
    if (a < 0.99) {
        discard;
    }
}

// 1 = lit by the sun, 0 = in shadow. Two cascades: the near one (sharp, around the camera)
// and the far one covering the rest of the visible street.
const SHADOW_OFFSETS: array<vec2<f32>, 16> = array<vec2<f32>, 16>(
    vec2<f32>(-0.942, -0.399), vec2<f32>(0.945, -0.769), vec2<f32>(-0.094, -0.929), vec2<f32>(0.345, 0.293),
    vec2<f32>(-0.915, 0.458), vec2<f32>(-0.815, -0.879), vec2<f32>(-0.382, 0.276), vec2<f32>(0.974, 0.756),
    vec2<f32>(0.443, -0.975), vec2<f32>(0.537, -0.474), vec2<f32>(-0.264, -0.418), vec2<f32>(0.792, -0.184),
    vec2<f32>(-0.758, 0.827), vec2<f32>(-0.386, -0.938), vec2<f32>(-0.203, 0.768), vec2<f32>(0.147, -0.169)
);

// Depth bias of the sun shadow, in metres along the sun's ray. The maps' depth runs over
// the light box's 2199 m, and the comparison used to take off 0.0015 (near) and 0.004 (far)
// of that: 3.3 m and 8.8 m. Nothing closer to the ground than that along the ray cast a
// shadow - not a car (1.4 m), not a bus (3 m; only its roof's shadow at a low sun,
// which lay detached and offset from the bus) - and the grass and kerbs had none either.
// The receiver plane below takes care of what the big constant stood in for.
const SHADOW_DEPTH_RANGE: f32 = 2199.0;
const SHADOW_BIAS_NEAR: f32 = 0.06;
const SHADOW_BIAS_FAR: f32 = 0.3;
const SHADOW_BIAS_CLOSE: f32 = 0.025;

// How the receiver's depth in the shadow map changes across the map (per unit of uv), from
// its normal: the PCF taps around the pixel compare against the surface's own depth at the
// tap instead of the centre's, so a surface at a slant to the sun does not shadow itself
// within the filter radius (acne), which a big constant bias used to hide. `lvp` is the
// cascade's light matrix (orthographic, so the map from the world is affine).
fn shadow_receiver_slope(lvp: mat4x4<f32>, n: vec3<f32>) -> vec2<f32> {
    let a = select(vec3<f32>(1.0, 0.0, 0.0), vec3<f32>(0.0, 1.0, 0.0), abs(n.x) > 0.9);
    let t1 = normalize(cross(n, a));
    let t2 = cross(n, t1);
    let p1 = (lvp * vec4<f32>(t1, 0.0)).xyz;
    let p2 = (lvp * vec4<f32>(t2, 0.0)).xyz;
    // (uv = 0.5 x + 0.5, 0.5 - 0.5 y)
    let d1 = vec3<f32>(p1.x * 0.5, -p1.y * 0.5, p1.z);
    let d2 = vec3<f32>(p2.x * 0.5, -p2.y * 0.5, p2.z);
    let det = d1.x * d2.y - d2.x * d1.y;
    if (abs(det) < 1e-12) {
        return vec2<f32>(0.0);
    }
    let g = vec2<f32>(d1.z * d2.y - d2.z * d1.y, d1.x * d2.z - d2.x * d1.z) / det;
    // a surface seen edge-on by the sun: keep the slope finite (it is in shadow of itself
    // there anyway, by its normal)
    return clamp(g, vec2<f32>(-2.0), vec2<f32>(2.0));
}

// The near map is an atlas two cascades wide: the near cascade in its left half, the close
// one (`camera.light_view_proj_close`) in its right half. `uv` is the cascade's own 0..1.
// The kernel's four outermost taps (SHADOW_OFFSETS 5, 1, 7, 12: one in each corner) and
// the one nearest its middle (15, which catches a pole's shadow thinner than the kernel)
// are taken first: when they agree - all lit or all in shadow - the pixel is not in a
// penumbra and the other eleven would agree too. Most of a picture is plainly lit or
// plainly shaded, so the filter mostly costs 5 compares instead of 16.
const SHADOW_CORNERS: array<i32, 5> = array<i32, 5>(5, 1, 7, 12, 15);
const SHADOW_REST: array<i32, 11> = array<i32, 11>(0, 2, 3, 4, 6, 8, 9, 10, 11, 13, 14);

// `scale`: the share of its half of the atlas the cascade fills from the top left (the close
// cascade is drawn no bigger than 2048 texels, see `SHADOW_CLOSE_MAX` in lib.rs).
fn shadow_pcf_atlas(uv: vec2<f32>, z: f32, slope: vec2<f32>, texel: f32, half: f32, spread: f32, bias: f32, scale: f32) -> f32 {
    var corners = 0.0;
    for (var k = 0; k < 5; k = k + 1) {
        let o = SHADOW_OFFSETS[SHADOW_CORNERS[k]] * texel * spread;
        let a = vec2<f32>((uv.x + o.x) * 0.5 * scale + half, (uv.y + o.y) * scale);
        corners = corners + textureSampleCompareLevel(t_shadow, s_shadow, a, z + dot(slope, o) - bias);
    }
    if (corners <= 0.0 || corners >= 5.0) {
        return corners * 0.2;
    }
    var sum = corners;
    for (var k = 0; k < 11; k = k + 1) {
        let o = SHADOW_OFFSETS[SHADOW_REST[k]] * texel * spread;
        let a = vec2<f32>((uv.x + o.x) * 0.5 * scale + half, (uv.y + o.y) * scale);
        sum = sum + textureSampleCompareLevel(t_shadow, s_shadow, a, z + dot(slope, o) - bias);
    }
    return sum / 16.0;
}

fn shadow_pcf_near(uv: vec2<f32>, z: f32, slope: vec2<f32>, texel: f32) -> f32 {
    return shadow_pcf_atlas(uv, z, slope, texel, 0.0, 1.6, SHADOW_BIAS_NEAR / SHADOW_DEPTH_RANGE, 1.0);
}

fn shadow_pcf_close(uv: vec2<f32>, z: f32, slope: vec2<f32>, texel: f32) -> f32 {
    // (camera.post.w: the close map's size over the near map's)
    let scale = select(1.0, camera.post.w, camera.post.w > 0.0);
    return shadow_pcf_atlas(uv, z, slope, texel / scale, 0.5, 1.8, SHADOW_BIAS_CLOSE / SHADOW_DEPTH_RANGE, scale);
}

fn shadow_push_close(n: vec3<f32>, ndl: f32) -> vec3<f32> {
    return n * (0.015 + 0.03 * (1.0 - ndl));
}

// The close cascade's value and weight at a point (weight 0 outside it): it fades into the
// near cascade between 60 and 90 % of its range from the camera.
fn shadow_close(world: vec3<f32>, n: vec3<f32>, ndl: f32, thin: bool) -> vec2<f32> {
    let range = camera.flags.w;
    if (range <= 0.0) {
        return vec2<f32>(1.0, 0.0);
    }
    let radial = distance(world, camera.cam_pos.xyz);
    let w = 1.0 - smoothstep(range * 0.6, range * 0.9, radial);
    if (w <= 0.001) {
        return vec2<f32>(1.0, 0.0);
    }
    let lp = camera.light_view_proj_close * vec4<f32>(world + shadow_push_close(n, ndl), 1.0);
    let uv = vec2<f32>(lp.x * 0.5 + 0.5, 0.5 - lp.y * 0.5);
    let edge = max(abs(uv.x - 0.5), abs(uv.y - 0.5));
    if (edge >= 0.48 || lp.z < 0.0 || lp.z > 1.0) {
        return vec2<f32>(1.0, 0.0);
    }
    let slope = select(shadow_receiver_slope(camera.light_view_proj_close, n), vec2<f32>(0.0), thin);
    return vec2<f32>(shadow_pcf_close(uv, lp.z, slope, camera.shadow.y), w);
}

fn shadow_pcf_far(uv: vec2<f32>, z: f32, slope: vec2<f32>, texel: f32) -> f32 {
    let bias = SHADOW_BIAS_FAR / SHADOW_DEPTH_RANGE;
    // (corners first, as in `shadow_pcf_atlas`)
    var corners = 0.0;
    for (var k = 0; k < 5; k = k + 1) {
        let o = SHADOW_OFFSETS[SHADOW_CORNERS[k]] * texel * 2.2;
        corners = corners + textureSampleCompareLevel(t_shadow_far, s_shadow, uv + o, z + dot(slope, o) - bias);
    }
    if (corners <= 0.0 || corners >= 5.0) {
        return corners * 0.2;
    }
    var sum = corners;
    for (var k = 0; k < 11; k = k + 1) {
        let o = SHADOW_OFFSETS[SHADOW_REST[k]] * texel * 2.2;
        sum = sum + textureSampleCompareLevel(t_shadow_far, s_shadow, uv + o, z + dot(slope, o) - bias);
    }
    return sum / 16.0;
}

// The shadow lookup point: pushed off the surface along its normal by about a texel of the
// cascade (0.14 m near, 0.68 m far), more where the surface turns away from the sun.
fn shadow_push_near(n: vec3<f32>, ndl: f32) -> vec3<f32> {
    return n * (0.06 + 0.12 * (1.0 - ndl));
}

fn shadow_push_far(n: vec3<f32>, ndl: f32) -> vec3<f32> {
    return n * (0.3 + 0.6 * (1.0 - ndl));
}

// `thin`: an alpha-tested card (leaves, fences), whose normal says nothing about its plane
// (OMSI points a tree's normals up): no receiver plane, the lookup moves half a metre
// towards the sun instead, which keeps a crown from speckling itself.
fn sun_shadow(world_in: vec3<f32>, n: vec3<f32>, thin: bool) -> f32 {
    if (camera.shadow.x < 0.5) {
        return 1.0;
    }
    let world = world_in + select(vec3<f32>(0.0), camera.sun_dir.xyz * 0.5, thin);
    let ndl = clamp(dot(n, camera.sun_dir.xyz), 0.0, 1.0);
    let close = shadow_close(world, n, ndl, thin);
    if (close.y >= 0.999) {
        return close.x;
    }
    let near_lp = camera.light_view_proj * vec4<f32>(world + shadow_push_near(n, ndl), 1.0);
    let near_uv = vec2<f32>(near_lp.x * 0.5 + 0.5, 0.5 - near_lp.y * 0.5);
    let inside = max(abs(near_uv.x - 0.5), abs(near_uv.y - 0.5));
    let t = camera.shadow.y;
    if (inside < 0.48 && near_lp.z >= 0.0 && near_lp.z <= 1.0) {
        let nv = shadow_pcf_near(near_uv, near_lp.z, select(shadow_receiver_slope(camera.light_view_proj, n), vec2<f32>(0.0), thin), t);
        return mix(nv, close.x, close.y);
    }
    let lp = camera.light_view_proj_far * vec4<f32>(world + shadow_push_far(n, ndl), 1.0);
    let uv = vec2<f32>(lp.x * 0.5 + 0.5, 0.5 - lp.y * 0.5);
    if (uv.x < 0.0 || uv.x > 1.0 || uv.y < 0.0 || uv.y > 1.0 || lp.z < 0.0 || lp.z > 1.0) {
        return 1.0;
    }
    let sum = shadow_pcf_far(uv, lp.z, select(shadow_receiver_slope(camera.light_view_proj_far, n), vec2<f32>(0.0), thin), t);
    let edge = clamp((0.5 - max(abs(uv.x - 0.5), abs(uv.y - 0.5))) * 12.0, 0.0, 1.0);
    return mix(1.0, sum, edge);
}

// Sum of the point lights registered in the grid cell of `p`. A light is registered in
// every cell its range touches, so the point's own cell holds every light that reaches it;
// looking into the neighbouring cells as well counted a light up to four times, and how
// often depended on where the cell edges fell - lamp pools brightened and dimmed as the
// camera moved.
// `map_k`: how much of the map's lamps a surface takes (0 on a light-mapped road in the
// classic picture, whose lamps are in its light map); a vehicle's own lights (dir.w -3, see
// lib.rs `gpu_light`) always shine - the headlights lit no road at all in vanilla - but only
// within their headlight's cone (axis dir.xyz, from extra.x behind the light, cosines of the
// outer and inner cone in extra.y/z): all round, they lit their own bus's front and saloon.
// The cone is seen from the nearest point of the line between the pair of headlamps, whose
// half width is -3 - dir.w: one beam from the axis looked like a single headlamp.
fn point_lights(p: vec3<f32>, n: vec3<f32>, map_k: f32) -> vec3<f32> {
    var sum = vec3<f32>(0.0);
    let cell = camera.light_grid.z;
    let side = u32(camera.light_grid.w);
    if (cell <= 0.0 || side == 0u) {
        return sum;
    }
    let f = (p.xy - camera.light_grid.xy) / cell;
    let x = i32(floor(f.x));
    let y = i32(floor(f.y));
    if (x >= 0 && y >= 0 && x < i32(side) && y < i32(side)) {
        let base = (u32(y) * side + u32(x)) * CELL_CAP;
        for (var j = 0u; j < CELL_CAP; j = j + 1u) {
            let li = grid[base + j];
            if (li == 0xffffffffu) {
                break;
            }
            let l = lights[li];
            let d = l.pos.xyz - p;
            let dist = length(d);
            if (dist >= l.pos.w) {
                continue;
            }
            // full intensity within 1/8 of the range, then inverse-square, cut off at the range;
            // the strength was set by eye while a street lamp was still counted about three
            // times (see above), so it is three times what it was, to keep the lamp pools
            let r0 = l.pos.w * 0.125;
            let att = min(1.0, (r0 * r0) / max(dist * dist, 0.01)) * clamp(1.0 - dist / l.pos.w, 0.0, 1.0) * 3.75;
            let ndl = max(dot(n, d / max(dist, 0.01)), 0.15);
            let headlight = l.dir.w < -2.5;
            var k = select(map_k, 1.0, headlight);
            if (headlight) {
                let apex = l.pos.xyz - l.dir.xyz * l.extra.x;
                let right = cross(l.dir.xyz, vec3<f32>(0.0, 0.0, 1.0));
                let side = right / max(length(right), 1e-3);
                let spread = -3.0 - l.dir.w;
                var to = p - apex;
                to = to - side * clamp(dot(to, side), -spread, spread);
                let cd = dot(to, l.dir.xyz) / max(length(to), 1e-3);
                k = k * smoothstep(l.extra.y, max(l.extra.z, l.extra.y + 0.01), cd);
            }
            sum = sum + l.color.rgb * l.color.w * att * ndl * k;
        }
    }
    return sum;
}

// [interiorlight]: the light of a vehicle's saloon lamps on a mesh that names them in its
// [illumination_interior], 1 = a lamp's full light on a seat under it. `code` is the
// instance's lamp code (lib.rs `Instance::interior_lamps`): the first of its lamp slots in
// `lights` times 64 plus how many; below 1 it is a plain brightness (a passenger standing in
// a lit bus). Each lamp is OMSI's Direct3D point light: attenuation
// 1 / (d² / core²) with `core` the lamp's [interiorlight] range, no cut-off short of 100 m,
// times N·L; the sum saturates, as Direct3D's vertex colour does.
fn interior_lamps(p: vec3<f32>, n: vec3<f32>, code: f32) -> vec3<f32> {
    if (code < 1.0) {
        return vec3<f32>(1.0, 0.96, 0.84) * clamp(code, 0.0, 1.0);
    }
    let c = u32(code + 0.5);
    // (LAMP_CODE_STRIDE: 64 - up to 63 lamps a mesh)
    let first = c >> 6u;
    let count = c & 63u;
    var sum = vec3<f32>(0.0);
    for (var i = 0u; i < count; i = i + 1u) {
        let l = lights[first + i];
        let r = l.extra.w;
        let d = l.pos.xyz - p;
        let dist2 = max(dot(d, d), 1e-4);
        if (l.color.w <= 0.0 || dist2 >= r * r) {
            continue;
        }
        let core = max(l.extra.y, 0.01);
        let att = core * core / dist2;
        let ndl = max(dot(n, d * inverseSqrt(dist2)), 0.0);
        sum = sum + l.color.rgb * l.color.w * att * ndl;
    }
    return min(sum, vec3<f32>(1.0));
}

// Value noise in three octaves: the "fractal" detail (the `detail_textures` setting) laid
// over the ground and the road surfaces close by, so that a blurred texture keeps some grain.
//
// The lattice cell is hashed as an integer. The old `fract(sin(dot(q, …)) * 43758.5)`
// hash took the cell's map coordinate - two and a half million at Spandau's 892 km - into
// a sine: in 32-bit floats its argument is a multiple of 64 there, and what came out was
// not noise but a ramp that repeated along straight lines, so every road wore thin
// diagonal streaks at an exact spacing. Cells are wrapped at `PATTERN_PERIOD` (the pattern
// coordinate is taken modulo that on the CPU, see `world_pattern_xy`), which keeps the
// hash's input small and the pattern seamless across the wrap.
const PATTERN_PERIOD: f32 = 1000.0;

fn hash_cell(c: vec2<f32>, cells: f32) -> f32 {
    // the cell index modulo the lattice's cells per period, as an exact integer
    let w = c - cells * floor(c / cells);
    var v = vec2<u32>(w) * 1664525u + vec2<u32>(1013904223u);
    // (a PCG-style mix: two rounds of multiply, cross-add and xor-shift)
    v.x = v.x + v.y * 1664525u;
    v.y = v.y + v.x * 1664525u;
    v = v ^ (v >> vec2<u32>(16u));
    v.x = v.x + v.y * 1664525u;
    v.y = v.y + v.x * 1664525u;
    v = v ^ (v >> vec2<u32>(16u));
    return f32(v.x >> 8u) / 16777215.0;
}

// `freq`: lattice cells per metre of `p` (a whole number of cells per `PATTERN_PERIOD`).
fn vnoise_f(p: vec2<f32>, freq: f32, offset: vec2<f32>) -> f32 {
    let q = p * freq + offset;
    let cells = round(PATTERN_PERIOD * freq);
    let i = floor(q);
    let f = q - i;
    let u = f * f * (3.0 - 2.0 * f);
    let a = hash_cell(i, cells);
    let b = hash_cell(i + vec2<f32>(1.0, 0.0), cells);
    let c = hash_cell(i + vec2<f32>(0.0, 1.0), cells);
    let d = hash_cell(i + vec2<f32>(1.0, 1.0), cells);
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}


fn detail_noise(p: vec2<f32>) -> f32 {
    // Anti-alias: point-sampling a lattice this fine aliases wherever one pixel's
    // footprint spans more than about one cell of it - looking down a street at a
    // grazing angle, or just driving away, constantly changes that footprint, and the
    // alias pattern with it, which reads as the "fractal" texture re-rendering itself as
    // the view angle changes. `fwidth` gives the footprint in the same units as `p`; an
    // octave fades to its own mean (0.5, the hash is uniform 0..1) once a pixel can no
    // longer resolve it, which is what an infinitely dense filter of it would give anyway.
    let w = max(fwidth(p).x, fwidth(p).y);
    let fade0 = 1.0 - smoothstep(0.3, 1.2, w * 2.9);
    let fade1 = 1.0 - smoothstep(0.3, 1.2, w * 0.73);
    let fade2 = 1.0 - smoothstep(0.3, 1.2, w * 0.19);
    let n0 = mix(0.5, vnoise_f(p, 2.9, vec2<f32>(0.0)), fade0);
    let n1 = mix(0.5, vnoise_f(p, 0.73, vec2<f32>(0.0)), fade1);
    let n2 = mix(0.5, vnoise_f(p, 0.19, vec2<f32>(0.0)), fade2);
    return n0 * 0.5 + n1 * 0.3 + n2 * 0.2;
}

fn world_pattern_xy(p: vec3<f32>) -> vec2<f32> {
    // Geometry is relative to the floating render origin for f32 precision. Procedural
    // patterns are not: reconstruct their stable map-space coordinate so crossing the
    // renderer's 100 m origin cell cannot re-seed the asphalt/ground pattern. The origin
    // arrives modulo PATTERN_PERIOD (lib.rs), so this stays a small, exact number: the map
    // coordinate itself (892 km at Spandau) has a 6 cm step in 32 bits, a fifth of the
    // finest noise cell.
    return p.xy + camera.world_origin.xy;
}

// Whether every component is a finite number, tested on the bits: Metal's fast maths takes
// `x != x` and comparisons with a NaN as it likes (a NaN colour happened to come out as
// something on the Mac), Direct3D 12 keeps the NaN and draws it black.
fn all_finite(v: vec3<f32>) -> bool {
    let e = bitcast<vec3<u32>>(v) & vec3<u32>(0x7f800000u);
    return all(e != vec3<u32>(0x7f800000u));
}

fn finite_or(v: vec3<f32>, fallback: vec3<f32>) -> vec3<f32> {
    return select(fallback, v, all_finite(v));
}

fn rain_hash(p: vec2<f32>) -> vec2<f32> {
    let q = vec2<f32>(dot(p, vec2<f32>(127.1, 311.7)), dot(p, vec2<f32>(269.5, 183.3)));
    return fract(sin(q) * 43758.5453);
}

// Raindrops on a window pane: the drops that sit on the glass (they land, grow and dry up
// again, each at its own pace) and the few that grow heavy enough to run down it, in fits
// and starts, leaving a trail of small droplets. `uv` is the pane's own texture coordinate
// (without the [texcoordtransY] scroll: that moved the whole picture of drops down at once,
// like paper off a roll), `wet` how wet the pane is. Scaled in metres over the glass and
// running down along the world's vertical as it lies on the pane, whatever way the
// model's texture is turned. Returns the drops' colour and cover.
fn rain_drops(world: vec3<f32>, uv: vec2<f32>, n: vec3<f32>, wet: f32, t: f32) -> vec4<f32> {
    if (wet <= 0.002) {
        return vec4<f32>(0.0);
    }
    // metres per unit of uv, and which way is down in uv
    let dpx = dpdx(world);
    let dpy = dpdy(world);
    let dux = dpdx(uv);
    let duy = dpdy(uv);
    let det = dux.x * duy.y - dux.y * duy.x;
    if (abs(det) < 1e-12) {
        return vec4<f32>(0.0);
    }
    // world position's derivative along u and v (inverse of the screen Jacobian)
    let dpdu = (dpx * duy.y - dpy * dux.y) / det;
    let dpdv = (dpy * dux.x - dpx * duy.x) / det;
    let mu = length(dpdu);
    let mv = length(dpdv);
    if (mu < 1e-6 || mv < 1e-6) {
        return vec4<f32>(0.0);
    }
    // quantised so neighbouring triangles of one pane share the grid
    let su = exp2(round(log2(mu)));
    let sv = exp2(round(log2(mv)));
    var p = vec2<f32>(uv.x * su, uv.y * sv);
    // down in uv: against the height's gradient
    var down = -vec2<f32>(dpdu.z / mu, dpdv.z / mv);
    if (length(down) < 0.2) {
        down = vec2<f32>(0.0, 1.0); // a roof light: no way down, the drops just sit
    }
    // Snapped to the nearest eighth of a turn: `down` comes from the world's vertical, and
    // the body rolls and pitches on its springs all the time - a grid turned with it by a
    // degree moves drops a metre from its origin by more than a cell, so every drop on the
    // pane jumped to a neighbour's place and back from frame to frame (the drops trembled
    // and flickered while driving). A pane's texture is laid out square to it, so the eighth
    // turns keep the drops fixed to the glass and still running down it.
    let ang = round(atan2(down.y, down.x) / 0.7853982) * 0.7853982;
    down = vec2<f32>(cos(ang), sin(ang));
    let side = vec2<f32>(down.y, -down.x);
    // pane coordinates in metres: x across, y down the glass
    let q = vec2<f32>(dot(p, side), dot(p, down));
    // a drop is a lens: it shows the world behind it upside down and small, which reads as
    // a darker rim with a bright spot where it catches the sky - not a white fleck
    var rim = 0.0;
    var glint = 0.0;
    var body = 0.0;
    // the drops that sit: two sizes on grids of 16 and 9 mm, one drop a cell at most
    for (var layer = 0; layer < 2; layer = layer + 1) {
        let cellsz = select(0.016, 0.009, layer == 1);
        let g = q / cellsz + vec2<f32>(f32(layer) * 17.3, f32(layer) * 5.1);
        let c = floor(g);
        let h = rain_hash(c);
        let life = 6.0 + h.y * 14.0;
        let ph = fract(t / life + h.x);
        // landed, full, drying: a drop comes and goes; more of them the wetter the glass
        let present = step(h.x, wet * select(0.55, 0.4, layer == 1)) * smoothstep(0.0, 0.05, ph) * (1.0 - smoothstep(0.8, 1.0, ph));
        let centre = c + 0.3 + 0.4 * rain_hash(c + 3.7);
        let r = (0.12 + 0.2 * h.y) * mix(0.7, 1.0, ph);
        let dv = (g - centre) * vec2<f32>(1.0, 0.85);
        let dist = length(dv) / max(r, 1e-3);
        let inside = (1.0 - smoothstep(0.8, 1.0, dist)) * present;
        body = max(body, inside);
        rim = max(rim, inside * smoothstep(0.45, 0.95, dist));
        // the sky's reflection: a small spot towards the top
        glint = max(glint, inside * (1.0 - smoothstep(0.0, 0.3, length(dv / max(r, 1e-3) - vec2<f32>(-0.25, -0.35)))));
    }
    // the runners: one lane every 6 cm, each with its own drop sliding down in jerks
    let lane_w = 0.06;
    let lane = floor(q.x / lane_w);
    let lh = rain_hash(vec2<f32>(lane, 7.0));
    if (lh.x < wet * 0.6) {
        let speed = 0.03 + 0.09 * lh.y;
        // stick and slip: the drop pauses and then hurries on
        let tt = t * speed + lh.x * 13.0;
        let y = (floor(tt) + smoothstep(0.35, 1.0, fract(tt))) * 0.35;
        let span = 1.5;
        let dy = fract((q.y - y) / span) * span; // how far above the drop's head
        let head_y = q.y - dy;
        let wobble = sin(head_y * 40.0 + lane) * 0.004;
        let x0 = (lane + 0.5 + (lh.y - 0.5) * 0.4) * lane_w + wobble;
        let dx = q.x - x0;
        // the head: a drop about 4 mm across, a little longer than wide
        let hd = length(vec2<f32>(dx, (dy - 0.004) * 0.7)) / 0.0028;
        let head = 1.0 - smoothstep(0.8, 1.0, hd);
        // the trail above it: a clear streak where the drop wiped the glass, beaded with
        // droplets, fading with the distance
        let trail_len = 0.1 + 0.25 * lh.y;
        let bead = step(0.6, fract(dy * 80.0 + lh.x * 5.0));
        let trail = (1.0 - smoothstep(0.0007, 0.0013, abs(dx))) * (1.0 - smoothstep(0.0, trail_len, dy)) * step(0.006, dy) * bead;
        // (the wiped streak behind the drop read as a drawn line down the glass in the
        // enhanced picture: only the head is drawn)
        _ = trail;
        body = max(body, head);
        rim = max(rim, head * smoothstep(0.45, 0.95, hd));
        glint = max(glint, head * (1.0 - smoothstep(0.0, 0.35, length(vec2<f32>(dx / 0.0028 + 0.25, (dy - 0.004) * 0.7 / 0.0028 + 0.35)))));
    }
    let k = clamp(wet * 1.5, 0.0, 1.0);
    let a = clamp(body * 0.22 + rim * 0.45 + glint * 0.55, 0.0, 1.0) * k;
    let col = (vec3<f32>(0.18, 0.2, 0.22) * rim * 0.45 + vec3<f32>(0.55, 0.58, 0.62) * body * 0.22 + vec3<f32>(1.0) * glint * 0.55) / max(a / max(k, 1e-3), 1e-3);
    return vec4<f32>(col, a);
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    if (material.emissive.w > 1.5) {
        // a pane's film of water: drops, not the sliding texture
        let wet = in.params.x;
        let d = rain_drops(in.world, in.uv - in.params.zw, safe_normal(in.normal), wet, camera.post.y);
        let light = camera.sun_color.rgb * camera.sun_dir.w * 0.5 + camera.sky_color.rgb + camera.ambient.rgb;
        // (fading out with the distance: see enhanced.wgsl - far off, black flecks)
        let near = 1.0 - smoothstep(4.0, 12.0, distance(in.world, camera.cam_pos.xyz));
        return vec4<f32>(d.rgb * clamp(light, vec3<f32>(0.05), vec3<f32>(1.2)), d.a * near);
    }
    var duv = in.uv;
    if (material.extra.x > 0.5) {
        // terrain: uv is tile space; the ground texture repeats extra.z times per tile
        duv = in.uv * material.extra.z;
    }
    var tex = textureSample(t_diffuse, s_diffuse, duv);
    if (material.extra.x > 0.5 && material.extra.y > 0.0) {
        // the ground texture's detail texture, repeated finer than the texture itself and
        // modulated over it as the original's terrain pass does. The stock detail maps are
        // bright (noise_low averages 242, gras_det 179), so they are meant to be multiplied
        // in plainly: doubling like a grey-centred D3D detail map blows the ground out.
        let det = textureSample(t_light, s_diffuse, in.uv * material.extra.y);
        tex = vec4<f32>(clamp(tex.rgb * det.rgb, vec3<f32>(0.0), vec3<f32>(1.0)), tex.a);
    }
    if (material.params.z > 0.5) {
        // [matl_transmap]: alpha comes from a separate map, its alpha channel (a map without one
        // is opaque, as D3D samples it: the WH UK AI cars' paint layer has a black 24-bit
        // `transmap_null.tga`, read as luminance the paint was invisible);
        // for terrain the map is the per-tile surface mask in tile space
        let tm = textureSample(t_trans, s_diffuse, in.uv);
        tex.a = select(1.0, tm.a, material.params.w > 0.5);
        if (material.extra.x > 0.5 && material.params.x > 1.5) {
            // A painted ground layer. The brush mask is coarse (0.6-3 m per texel) and
            // binary; the loader smooths it into a soft ramp around a smooth curve
            // (`smooth_paint_mask` in scene.rs). Sharpen the ramp and let the ground
            // texture's own light and dark decide where the new surface wins within the
            // ramp - cobbles then fray into the grass stone by stone instead of in soft
            // rectangles. (On the raw mask this sharpening is what drew the texel grid
            // as a staircase along every painted edge.)
            let lum = dot(tex.rgb, vec3<f32>(0.333, 0.333, 0.333));
            let m = tex.a + (lum - 0.5) * 0.45;
            tex.a = smoothstep(0.32, 0.68, m);
        }
    }
    let mode = material.params.x;
    // Mip-filtered alpha is the fraction of the pixel covered by leaves or fence wires.
    // Tighten the transition around the cutout edge before MSAA turns it into sample
    // coverage. The depth prepass leaves these draws out so its binary cutoff cannot hide
    // the scene behind samples that the colour pass leaves open.
    if (ALPHA_TEST && mode > 0.5 && mode < 1.5) {
        if (ALPHA_TO_COVERAGE) {
            let aa = max(fwidth(tex.a) * 0.5, 1.0 / 255.0);
            if (tex.a < 0.5 - aa) {
                discard;
            }
            tex.a = smoothstep(0.5 - aa, 0.5 + aa, tex.a);
        } else if (tex.a < 0.5) {
            discard;
        }
    }
    let n = safe_normal(in.normal);
    let ndl = max(dot(n, camera.sun_dir.xyz), 0.0);
    // three lights like the original: direct sun (A), light from above (B), ambient (C)
    let from_above = 0.5 + 0.5 * n.z;
    let shadow = sun_shadow(in.world, n, mode > 0.5 && mode < 1.5);
    // screen-space ambient occlusion darkens the indirect light (sky and ambient) in
    // corners, under the bus, between the seats - not the sun, which the shadow map handles
    var ao = 1.0;
    // (not on a blended surface: the AO is the opaque depth's, and a translucent door
    // showed the shade of what stood behind it)
    if (camera.clouds.w > 0.5 && mode < 1.5) {
        ao = ao_at(in.clip.xy, in.world);
    }
    var diffuse = camera.sun_color.rgb * camera.sun_dir.w * ndl * shadow + (camera.sky_color.rgb * from_above * (0.6 + 0.4 * shadow) + camera.ambient.xyz) * ao;
    var albedo = tex.rgb;
    if (camera.flags.x > 0.5 && (material.extra.x > 0.5 || in.params2.w > 0.5)) {
        // detail texturing (setting): procedural grain on the ground and the roads, fading out with distance
        let dist0 = distance(in.world, camera.cam_pos.xyz);
        let k = clamp(1.0 - (dist0 - 25.0) / 120.0, 0.0, 1.0);
        let nz = detail_noise(world_pattern_xy(in.world));
        albedo = albedo * (1.0 + (nz - 0.5) * 0.42 * k);
    }
    // ([nomaplighting]: params.y 0.25 - not lit by the map's lamps)
    // (a light-mapped road or plate takes the map's lamps only in Vanilla+: as OMSI 2 shows
    // it, the vanilla picture lights it from the tile light map alone)
    let lm_only = light_map_mapped(material.params) && camera.sky_color.w > 0.5;
    let map_lamps = select(1.0, 0.0, (material.params.y > 0.2 && material.params.y < 0.3) || lm_only);
    let lamp_light = point_lights(in.world, n, map_lamps);
    var lit = albedo * material.color.rgb * (diffuse + lamp_light);
    if (material.specular.w > 0.0 && material.params.y < 0.5) {
        // the D3D material's own highlight (specular colour and power of the o3d file or a
        // [matl_allcolor]) from the sun, added after the texture as D3D's specular is;
        // only on the side that faces the sun, and not in its shadow
        let vdir_s = normalize(camera.cam_pos.xyz - in.world);
        let hs = normalize(vdir_s + camera.sun_dir.xyz);
        let towards_sun = clamp(ndl * 4.0, 0.0, 1.0);
        lit = lit + material.specular.rgb * camera.sun_color.rgb * camera.sun_dir.w * pow(max(dot(n, hs), 0.0), material.specular.w) * shadow * towards_sun;
    }
    if (material.params.y > 0.5) {
        lit = tex.rgb * material.color.rgb;
    }
    lit = lit + tex.rgb * material.emissive.rgb;
    // the tile light map, as on the terrain: the lamps' pools on the roads and the plates,
    // lighting the surface (not painted over it: added as it was, the pool lay on the road
    // as a white patch); only where it is their light at night - elsewhere the map's lamps
    // light them as they light the squares and pavements beside them
    if (lm_only) {
        lit = lit + albedo * material.color.rgb * light_map_at(in.world) * camera.sun_color.w;
    }
    // [interiorlight]: the saloon lamps on the meshes and passengers they illuminate
    lit = lit + tex.rgb * interior_lamps(in.world, n, in.params2.z);
    if (material.extra.w > 0.5) {
        // [matl_nightmap]: self-illumination that fades in with the night
        // terrain: the tile light map in tile space (north at the top row)
        let nuv = select(duv, vec2<f32>(in.uv.x, 1.0 - in.uv.y), material.extra.x > 0.5);
        let nm = textureSample(t_night, s_diffuse, nuv);
        // a [matl_item] night map is switched by its variable (warning lamps, displays):
        // it glows whenever that is on, by day as well; the others fade in with the night
        let night = select(camera.sun_color.w, 1.0, material.extra.w > 1.5);
        lit = lit + nm.rgb * night * select(clamp(in.params2.y, 0.0, 1.0), 1.0, material.extra.w > 1.5);
    }
    if (material.params2.x > 0.5 && material.extra.x < 0.5) {
        // [matl_lightmap]: a light mask (interior lighting) multiplied with the diffuse
        // texture, scaled by a script variable
        let lm = textureSample(t_light, s_diffuse, duv);
        lit = lit + tex.rgb * lm.rgb * clamp(in.params2.x, 0.0, 1.0);
    }
    // a pane's reflection, laid over what shows through it (see the end)
    var pane_refl = vec3<f32>(0.0);
    var pane_k = 0.0;
    if (material.params2.y > 0.0) {
        // [matl_envmap]: sphere map reflection, masked by the diffuse alpha like the original
        let vdir = normalize(in.world - camera.cam_pos.xyz);
        let r = reflect(vdir, n);
        let rx = dot(r, camera.cam_right.xyz);
        let ry = dot(r, camera.cam_up.xyz);
        // paint reflects a soft image; glass keeps the sphere map sharp. Sampled sharp,
        // the trees photographed into envmap.bmp showed up as camouflage on the body.
        var env_uv = vec2<f32>(rx * 0.5 + 0.5, 0.5 + ry * 0.5);
        if (material.bump.y > 0.5) {
            env_uv = env_uv + bump_offset(duv);
        }
        // A blended transmap body is a masked paint surface, not glass. Traffic cars
        // commonly use this form for the body; only an actual depth-disabled blend is
        // treated as a window/transparent surface.
        let glass = material.params.x > 1.5 && material.bump.z > 0.5 &&
            (material.params2.y > 0.0 || material.params.z > 0.5 || material.emissive.w > 0.5);
        let env = textureSampleBias(t_env, s_diffuse, env_uv, select(2.0, 0.0, glass));
        let diffuse_a = textureSample(t_diffuse, s_diffuse, duv).a;
        // strength: reflection mask x factor; the factor saturates at 1 like a D3D texture
        // factor (the SD202 body writes 10 for "full": its paint alpha of 0.03-0.08 is the
        // gloss, and the far LOD's own mask of 0.05-0.12 matches that, not ten times it),
        // capped so transparent glass keeps its tint
        // Glass needs a visible normal-incidence reflection as well as the stronger
        // grazing-angle reflection. The old 0.22/0.05 combination made bus windows look
        // like pale uncoated plastic when viewed from the driver's seat.
        let factor = max(min(material.params2.y, 1.0), select(0.0, 0.25, glass));
        // (a blended pane's alpha is its transparency, not a reflection mask: read as one,
        // the Scania's nearly clear panes reflected nothing at all)
        var k = clamp(factor * reflection_mask(duv, select(diffuse_a, 1.0, glass)), 0.0, 1.0) * select(1.0, 0.65, glass);
        // (paint reflects too, as much as above: its mask is the gloss the texture's alpha
        // or [matl_envmap_mask] gives, the factor saturating at 1 - the SD202's 10 over a
        // paint alpha of a few per cent is a soft sheen. Left out for painted bodies, every
        // bus was matt; taken unsaturated, the SD202 became a mirror.)
        if (glass) {
            // See-through glass reflects a few per cent of the sphere map when you look
            // straight through it and much more at a grazing angle - without that the
            // windscreen carried an even milky veil over the whole road ahead.
            // (seen from inside the glass the normal points away, so take the angle
            // either way round)
            let facing = clamp(abs(dot(vdir, n)), 0.0, 1.0);
            k = k * (0.18 + 0.82 * pow(1.0 - facing, 4.0));
        }
        // the sphere map was photographed by day: dim it with the scene light at night
        // (outside the classic picture it goes with the night as well: the photo's sunlit
        // trees and blue sky kept a fifth of their light at midnight, and the mirrors of
        // an enhanced session - drawn with this shader - showed a street by daylight in
        // every pane they caught)
        let env_night = select(1.0 - 0.85 * clamp(camera.sun_color.w, 0.0, 1.0), 1.0, camera.sky_color.w > 0.5);
        let env_light = clamp(camera.sun_color.r * camera.sun_dir.w + camera.sky_color.r + camera.ambient.r, 0.05, 1.0) * env_night;
        if (material.params.x < 1.5) {
            // rain: a painted body goes darker and glossier when it is wet, so the bus
            // stands out against a grey street instead of fading into it
            let wet = camera.shadow.w * weather_outside_n(in.world, n, false, in.params2.w);
            lit = lit * (1.0 - 0.30 * wet);
            // the water film mirrors the (overcast) sky a little, mostly at grazing
            // angles; kept faint - a strong rim read as a white outline round the bus
            let facing = clamp(abs(dot(vdir, n)), 0.0, 1.0);
            let sheen = wet * min(material.params2.y, 1.0) * (0.05 + 0.22 * pow(1.0 - facing, 5.0));
            lit = mix(lit, camera.sky_color.rgb * env_light, sheen);
        }
        if (glass) {
            pane_refl = env.rgb * env_light;
            // the inner face of the bus's own glass mirrors the dark cab, not the sky (see
            // enhanced.wgsl): the doors seen from the driver's seat were a grey veil
            pane_k = k * (1.0 - 0.85 * near_player_vehicle(in.world) * inside_vehicle(camera.cam_pos.xyz));
        } else {
            // Omsi.exe's environment stage (0x7fd6c4 at 0x7ff626, fixed function): the sphere
            // map's colour as it is, over what the stages before made of the lit texture -
            // D3DTOP_LERP by the texture factor when the material has neither a
            // [matl_transmap] nor a [matl_envmap_mask], D3DTOP_BLENDCURRENTALPHA when it
            // has one. The factor is [matl_envmap]'s times the ambient light plus `g` (the
            // light the vehicle stands in: the lamps of the tile's light map and the sky),
            // each at most 1, packed into a D3DCOLOR without saturation (0x7ff8ed: a
            // factor over 1 spills its bits into the next channel, as there). The alpha
            // it blends by is the mask's, or the diffuse texture's times `g`
            // (0x7feacc, 0x7ff092) - never the diffuse alpha of a material without one of
            // them: taken as a reflection mask, a body whose texture carries an alpha
            // channel for other purposes was mirrored (dark) where that alpha was high.
            // `g` (0x861ca0, set per vehicle at 0x7d8735): the mean of the light the tile's
            // light map throws on it plus the day's light A (weather +0xac = the mean of
            // lightcolor A, 0x75333f - by it the stars fade), at most 1
            let g = clamp(dot(lamp_light, vec3<f32>(1.0 / 3.0)) + dot(camera.sun_color.rgb, vec3<f32>(1.0 / 3.0)), 0.0, 1.0);
            var kk = vec3<f32>(0.0);
            if (has_transmap_declared() || has_env_mask()) {
                let a = select(diffuse_a, textureSample(t_envmask, s_diffuse, duv).a, has_env_mask());
                kk = vec3<f32>(a * g);
            } else {
                kk = omsi_texture_factor(material.params2.y, camera.ambient.rgb + vec3<f32>(g));
            }
            lit = mix(lit, env.rgb, kk);
        }
    }
    // wet road: a surface whose texture carries [moisture] darkens under rain and starts
    // to mirror the sky, strongest where you look along it (the Fresnel sheen that makes a
    // wet street read as wet)
    let outside = weather_outside_n(in.world, n, material.extra.x > 0.5, in.params2.w);
    let wet = camera.shadow.w * material.params2.z * outside;
    if (wet > 0.0) {
        let vdir = normalize(in.world - camera.cam_pos.xyz);
        let facing = clamp(-dot(vdir, n), 0.0, 1.0);
        let fresnel = pow(1.0 - facing, 4.0);
        lit = lit * mix(1.0, 0.55, wet);
        let sheen = camera.sky_color.rgb * 0.5 + camera.sun_color.rgb * camera.sun_dir.w * 0.35;
        lit = mix(lit, sheen, clamp(fresnel * wet * 0.85, 0.0, 0.8));
    }
    // snow: the ground, the roads and every upward-facing surface whiten under it
    // (not on a shadow blob: whitened, it lit the snow under the bus instead of shading it)
    // (not in vanilla: OMSI 2 shows snow only through the season's WinterSnow textures)
    let snow = camera.ambient.w * outside * select(1.0, 0.0, in.params2.w > 1.5 || camera.sky_color.w > 0.5);
    if (snow > 0.0) {
        let up = clamp(n.z, 0.0, 1.0);
        let ground = select(0.0, 1.0, material.extra.x > 0.5 || material.params2.z > 0.0);
        // only surfaces that really face up get a cover; a soft threshold keeps the snow
        // off the sides and off the grazing rims that showed as a white outline
        let cover = snow * clamp(max(ground, smoothstep(0.78, 0.95, up) * 0.8), 0.0, 1.0);
        let light = camera.sun_color.rgb * camera.sun_dir.w * ndl * shadow * 0.6 + camera.sky_color.rgb * 0.7 + camera.ambient.xyz;
        let white = vec3<f32>(0.92, 0.94, 0.98) * light * ao;
        lit = mix(lit, white, cover * (0.55 + 0.35 * tex.a));
    }
    let dist = distance(in.world, camera.cam_pos.xyz);
    let f = 1.0 - exp(-fog_distance(in.world) * camera.fog.w);
    var rgb = mix(lit, camera.fog.xyz, clamp(f, 0.0, 1.0));
    if (camera.flags.z > 0.0) {
        // Never taken: flags.z (the old enhanced look's aerial perspective) is always 0
        // now - the enhanced path has its own fragment shader (enhanced.wgsl). The branch
        // stays because Metal's fast-math contracts the fog mix above differently without
        // it, and the vanilla picture is to stay what it was to the last code value.
        let vdir = normalize(in.world - camera.cam_pos.xyz);
        let towards_sun = clamp(dot(vdir, camera.sun_dir.xyz), 0.0, 1.0);
        let k = (1.0 - exp(-dist * 0.00045)) * camera.flags.z * (0.55 + 0.45 * clamp(camera.sun_dir.w, 0.0, 1.0));
        let scatter = mix(camera.sky_color.rgb * 1.4 + camera.ambient.rgb * 0.5, camera.sun_color.rgb * camera.sun_dir.w + camera.sky_color.rgb, towards_sun * towards_sun);
        rgb = mix(rgb, scatter, clamp(k * 0.6, 0.0, 0.5));
    }
    // (a NaN anywhere above - a zero-length vector normalised, 0 x infinity - was a black
    // patch on Windows, e.g. an EN92's headlights while off, and invisible on the Mac)
    rgb = finite_or(rgb, finite_or(albedo * material.color.rgb * diffuse, albedo));
    var a = tex.a * material.color.a;
    if (mode < 0.5) {
        a = 1.0;
    }
    a = a * in.params.x;
    if (pane_k > 0.0 && mode > 1.5) {
        // the reflection lies on top of the pane: at the pane's own faint alpha it
        // vanished with it; the blend keeps it where the glass itself is clear
        let a2 = clamp(a + (1.0 - a) * pane_k, a, 1.0);
        let refl = mix(pane_refl, camera.fog.xyz, clamp(f, 0.0, 1.0));
        return vec4<f32>((rgb * a + refl * pane_k) / max(a2, 1e-3), a2);
    }
    return vec4<f32>(rgb, a);
}
