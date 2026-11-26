//! Shared open Wavefront `.obj` 3D mesh loader for Raw `wgpu` & Bevy 3D viewports.

/// Open Utah-style teapot Wavefront `.obj` model embedded into the binary cleanly.
pub const SHARED_OPEN_MODEL_OBJ: &str = include_str!("../assets/open_teapot.obj");
/// Human-readable identifier displayed in both 3D viewport modes.
pub const SHARED_OPEN_MODEL_NAME: &str = "open_teapot.obj";

/// Single interleaved 3D mesh vertex (`position` + `normal`, 24 bytes).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MeshVertex {
    /// Object-space 3D coordinate normalized within unit sphere.
    pub pos: [f32; 3],
    /// Smooth outward surface normal.
    pub normal: [f32; 3],
}

/// Indexed triangle mesh parsed from a Wavefront `.obj` source string.
#[derive(Clone, Debug)]
pub struct LoadedMesh {
    /// Vertex buffer entries.
    pub vertices: Vec<MeshVertex>,
    /// Triangle index triplets (`len % 3 == 0`).
    pub indices: Vec<u32>,
}

fn parse_vertex_ref(
    token: &str,
    v_count: usize,
    vn_count: usize,
) -> Option<(usize, Option<usize>)> {
    let mut parts = token.split('/');
    let v_raw = parts.next()?.parse::<isize>().ok()?;
    let v_idx = resolve_obj_index(v_raw, v_count)?;
    let _vt = parts.next();
    let vn_idx = parts
        .next()
        .filter(|s| !s.is_empty())
        .and_then(|s| s.parse::<isize>().ok())
        .and_then(|i| resolve_obj_index(i, vn_count));
    Some((v_idx, vn_idx))
}

fn resolve_obj_index(raw: isize, len: usize) -> Option<usize> {
    let resolved = if raw > 0 {
        raw - 1
    } else if raw < 0 {
        len as isize + raw
    } else {
        return None;
    };
    usize::try_from(resolved).ok().filter(|&i| i < len)
}

fn add_flat_fallback_normals(
    positions: &[[f32; 3]],
    tris: &[(usize, usize, usize)],
) -> Vec<[f32; 3]> {
    let mut accum = vec![[0.0_f32; 3]; positions.len()];
    for &(a, b, c) in tris {
        let (pa, pb, pc) = (positions[a], positions[b], positions[c]);
        let e1 = [pb[0] - pa[0], pb[1] - pa[1], pb[2] - pa[2]];
        let e2 = [pc[0] - pa[0], pc[1] - pa[1], pc[2] - pa[2]];
        let n = [
            e1[1] * e2[2] - e1[2] * e2[1],
            e1[2] * e2[0] - e1[0] * e2[2],
            e1[0] * e2[1] - e1[1] * e2[0],
        ];
        for idx in [a, b, c] {
            accum[idx][0] += n[0];
            accum[idx][1] += n[1];
            accum[idx][2] += n[2];
        }
    }
    accum
        .into_iter()
        .map(|n| {
            let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
            if len > 1e-6 {
                [n[0] / len, n[1] / len, n[2] / len]
            } else {
                [0.0, 1.0, 0.0]
            }
        })
        .collect()
}

/// Parses Wavefront `.obj` text (`v`, `vn`, `f` with triangles/quads/n-gons) into a [`LoadedMesh`].
pub fn parse_wavefront_obj(src: &str) -> LoadedMesh {
    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut normals: Vec<[f32; 3]> = Vec::new();
    let mut tri_corners: Vec<(usize, Option<usize>)> = Vec::new();
    for raw in src.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut toks = line.split_whitespace();
        match toks.next() {
            Some("v") => {
                let coords: Vec<f32> = toks.take(3).filter_map(|t| t.parse().ok()).collect();
                if coords.len() == 3 {
                    positions.push([coords[0], coords[1], coords[2]]);
                }
            }
            Some("vn") => {
                let coords: Vec<f32> = toks.take(3).filter_map(|t| t.parse().ok()).collect();
                if coords.len() == 3 {
                    normals.push([coords[0], coords[1], coords[2]]);
                }
            }
            Some("f") => {
                let poly: Vec<_> = toks
                    .filter_map(|t| parse_vertex_ref(t, positions.len(), normals.len()))
                    .collect();
                for win in poly.windows(2).skip(1) {
                    tri_corners.push(poly[0]);
                    tri_corners.push(win[0]);
                    tri_corners.push(win[1]);
                }
            }
            _ => {}
        }
    }
    build_indexed_mesh(&positions, &normals, &tri_corners)
}

fn build_indexed_mesh(
    positions: &[[f32; 3]],
    normals: &[[f32; 3]],
    corners: &[(usize, Option<usize>)],
) -> LoadedMesh {
    let mut tris = Vec::with_capacity(corners.len() / 3);
    for ch in corners.as_chunks::<3>().0 {
        tris.push((ch[0].0, ch[1].0, ch[2].0));
    }
    let fallback = if normals.is_empty() {
        add_flat_fallback_normals(positions, &tris)
    } else {
        Vec::new()
    };
    let mut vertices = Vec::with_capacity(positions.len());
    let mut indices = Vec::with_capacity(corners.len());
    if corners.iter().all(|&(vi, ni)| ni == Some(vi)) && normals.len() == positions.len() {
        for i in 0..positions.len() {
            vertices.push(MeshVertex {
                pos: positions[i],
                normal: normals[i],
            });
        }
        indices.extend(corners.iter().map(|&(vi, _)| vi as u32));
    } else {
        for &(vi, ni) in corners {
            let n = ni
                .and_then(|idx| normals.get(idx).copied())
                .or_else(|| fallback.get(vi).copied())
                .unwrap_or([0.0, 1.0, 0.0]);
            let idx = vertices.len() as u32;
            vertices.push(MeshVertex {
                pos: positions[vi],
                normal: n,
            });
            indices.push(idx);
        }
    }
    LoadedMesh { vertices, indices }
}

/// Loads the embedded shared open 3D model (`open_teapot.obj`).
pub fn load_shared_open_model() -> LoadedMesh {
    parse_wavefront_obj(SHARED_OPEN_MODEL_OBJ)
}

/// Safely serializes [`MeshVertex`] entries into tight little-endian bytes (`24` bytes/vertex).
pub fn pack_mesh_vertices_le(verts: &[MeshVertex]) -> Vec<u8> {
    let mut out = Vec::with_capacity(verts.len() * 24);
    for v in verts {
        for f in v.pos {
            out.extend_from_slice(&f.to_le_bytes());
        }
        for f in v.normal {
            out.extend_from_slice(&f.to_le_bytes());
        }
    }
    out
}
