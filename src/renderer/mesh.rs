//! Vertex types and procedural mesh generation.

use bytemuck::{Pod, Zeroable};

/// Position + normal, used for the solid globe (and reused, position-only,
/// for the atmosphere shell).
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
pub struct VertexPN {
    pub pos: [f32; 3],
    pub nrm: [f32; 3],
}

/// Position + colour, used for coastline / border line segments.
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
pub struct VertexPC {
    pub pos: [f32; 3],
    pub col: [f32; 3],
}

/// Build a UV sphere. `stacks` are latitude bands, `sectors` are longitude
/// divisions. Returns vertices (position + outward normal) and triangle
/// indices.
pub fn uv_sphere(stacks: u32, sectors: u32, radius: f32) -> (Vec<VertexPN>, Vec<u32>) {
    let mut verts = Vec::with_capacity(((stacks + 1) * (sectors + 1)) as usize);
    let mut indices = Vec::with_capacity((stacks * sectors * 6) as usize);

    for i in 0..=stacks {
        let phi = (i as f32 / stacks as f32) * std::f32::consts::PI; // 0..PI (pole to pole)
        let (sp, cp) = phi.sin_cos();
        for j in 0..=sectors {
            let theta = (j as f32 / sectors as f32) * std::f32::consts::TAU; // 0..2PI
            let (st, ct) = theta.sin_cos();
            // Unit direction; y is the polar axis.
            let n = [sp * ct, cp, sp * st];
            verts.push(VertexPN {
                pos: [n[0] * radius, n[1] * radius, n[2] * radius],
                nrm: n,
            });
        }
    }

    let cols = sectors + 1;
    for i in 0..stacks {
        for j in 0..sectors {
            let a = i * cols + j;
            let b = a + cols;
            indices.extend_from_slice(&[a, b, a + 1, a + 1, b, b + 1]);
        }
    }

    (verts, indices)
}
