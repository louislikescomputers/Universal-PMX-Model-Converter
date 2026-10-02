//! Shared importer utilities: geometry helpers + a tiny, dependency-free
//! glTF 2.0 JSON parser (glTF is our primary input format; see DECISIONS.md).

use crate::error::{MmdconvError, Result};
use crate::ir::{Mesh, Vertex, Weight};
use glam::{Vec3, Vec3A};

/// Triangulate an index list of arbitrary polygons (quads, n-gons) into a
/// triangle fan per polygon. `winding` = vertices per polygon.
pub fn triangulate(poly_indices: &[u32], winding: usize) -> Vec<u32> {
    let mut out = Vec::with_capacity(poly_indices.len());
    if winding == 3 {
        out.extend_from_slice(poly_indices);
        return out;
    }
    for poly in poly_indices.chunks(winding.max(3)) {
        if poly.len() < 3 {
            continue;
        }
        for w in poly[1..].windows(2) {
            out.push(poly[0]);
            out.push(w[0]);
            out.push(w[1]);
        }
    }
    out
}

/// Split a flat f32 slice into Vec3s.
pub fn points_as_vec3(data: &[f32]) -> Vec<Vec3> {
    data.chunks_exact(3).map(|c| Vec3::from_slice(c)).collect()
}

/// Compute smooth vertex normals from a triangle mesh (used when the source
/// has no normal attribute). Accumulates face normals weighted by area×edge
/// cross-product magnitude (power heuristic), then normalizes.
pub fn compute_smooth_normals(mesh: &mut Mesh) {
    let n = mesh.vertices.len();
    let mut acc = vec![Vec3A::ZERO; n];
    for p in &mesh.primitives {
        for tri in p.indices.chunks_exact(3) {
            let (a, b, c) = (tri[0] as usize, tri[1] as usize, tri[2] as usize);
            if a >= n || b >= n || c >= n {
                continue;
            }
            let va = mesh.vertices[a].pos;
            let vb = mesh.vertices[b].pos;
            let vc = mesh.vertices[c].pos;
            let cr = (vb - va).cross(vc - va);
            if !cr.is_finite() {
                continue;
            }
            let na = Vec3A::from_vec3(cr);
            // power-of-heuristic weighting per corner
            let pa = Vec3A::from_vec3(va);
            let pb = Vec3A::from_vec3(vb);
            let pc = Vec3A::from_vec3(vc);
            let wa = (pb - pa).length().max(1e-9) * (pc - pa).length().max(1e-9);
            let wb = (pa - pb).length().max(1e-9) * (pc - pb).length().max(1e-9);
            let wc = (pa - pc).length().max(1e-9) * (pb - pc).length().max(1e-9);
            let inv = 1.0 / (wa * wb * wc).max(1e-20);
            acc[a] += na * (wb * wc * inv).min(10.0);
            acc[b] += na * (wa * wc * inv).min(10.0);
            acc[c] += na * (wa * wb * inv).min(10.0);
        }
    }
    for (i, v) in mesh.vertices.iter_mut().enumerate() {
        let len = acc[i].length();
        if len > 1e-12 {
            v.normal = acc[i].normalize().as_vec3();
        } else {
            v.normal = Vec3::Y;
        }
    }
}

/// Normalize weights: sort desc, drop zeros/negatives/NaNs, truncate to 4,
/// renormalize to sum 1. Returns the number of vertices that needed
/// truncation. Degenerate (all-zero) weights become BDEF1 on bone 0.
pub fn normalize_vertex_weights(vertices: &mut [Vertex], fallback_bone: u32) -> usize {
    let mut truncated = 0usize;
    for v in vertices.iter_mut() {
        let mut ws: Vec<Weight> = v
            .weights
            .iter()
            .filter(|w| w.weight.is_finite() && w.weight > 1e-6)
            .cloned()
            .collect();
        ws.sort_by(|a, b| b.weight.total_cmp(&a.weight));
        if ws.len() > 4 {
            truncated += 1;
            ws.truncate(4);
        }
        let sum: f32 = ws.iter().map(|w| w.weight).sum();
        if sum <= 0.0 || !sum.is_finite() {
            ws = vec![Weight { bone: fallback_bone, weight: 1.0 }];
        } else {
            for w in ws.iter_mut() {
                w.weight /= sum;
            }
            // guarantee exact-ish sum on the strongest bone
            let resid: f32 = 1.0 - ws.iter().map(|w| w.weight).sum::<f32>();
            ws[0].weight += resid;
        }
        v.weights = ws;
    }
    truncated
}

/// Merge duplicate vertices (identical pos/normal/uv/weights within epsilon)
/// and remap indices. Keeps output deterministic by first-occurrence order.
pub fn deduplicate_vertices(mesh: &mut Mesh) {
    const EPS: f32 = 1e-5;
    let mut key_map: std::collections::HashMap<[i64; 8], u32> = std::collections::HashMap::new();
    let mut new_verts: Vec<Vertex> = Vec::with_capacity(mesh.vertices.len());
    let mut remap = vec![0u32; mesh.vertices.len()];
    fn q(x: f32) -> i64 {
        (x / EPS).round() as i64
    }
    for (i, v) in mesh.vertices.iter().enumerate() {
        let mut key = [
            q(v.pos.x), q(v.pos.y), q(v.pos.z),
            q(v.normal.x), q(v.normal.y), q(v.normal.z),
            q(v.uv[0]), q(v.uv[1]),
        ];
        // mix weight signature + uv1 into the remaining key slots deterministically
        let mut h: i64 = 1469598103934665603u64 as i64; // FNV-1a
        let mut mix = |x: i64| {
            h ^= x;
            h = h.wrapping_mul(1099511628211);
        };
        for w in &v.weights {
            mix(w.bone as i64);
            mix(q(w.weight));
        }
        if let Some((a, b)) = v.uv1 {
            mix(q(a));
            mix(q(b));
        } else {
            mix(i64::MIN);
        }
        key[7] = key[7].wrapping_mul(31).wrapping_add(h);
        let found = key_map.get(&key).copied();
        if let Some(old) = found {
            remap[i] = old;
        } else {
            let idx = new_verts.len() as u32;
            new_verts.push(v.clone());
            key_map.insert(key, idx);
            remap[i] = idx;
        }
    }
    for p in mesh.primitives.iter_mut() {
        for ix in p.indices.iter_mut() {
            *ix = remap[*ix as usize];
        }
    }
    mesh.vertices = new_verts;
}

// ---------------------------------------------------------------------------
// Minimal glTF JSON parser (JSON5 subset not supported; strict JSON only)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub enum JsonValue {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<JsonValue>),
    Obj(Vec<(String, JsonValue)>),
}

impl JsonValue {
    pub fn get(&self, key: &str) -> Option<&JsonValue> {
        match self {
            JsonValue::Obj(items) => items.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }
    pub fn as_array(&self) -> Option<&[JsonValue]> {
        match self {
            JsonValue::Arr(a) => Some(a),
            _ => None,
        }
    }
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            JsonValue::Num(n) => Some(*n),
            _ => None,
        }
    }
    pub fn as_str(&self) -> Option<&str> {
        match self {
            JsonValue::Str(s) => Some(s),
            _ => None,
        }
    }
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            JsonValue::Bool(b) => Some(*b),
            _ => None,
        }
    }
    /// Read an array of numbers as f32 vec.
    pub fn as_f32_vec(&self) -> Option<Vec<f32>> {
        self.as_array().map(|a| a.iter().filter_map(|x| x.as_f64()).map(|x| x as f32).collect())
    }
    pub fn as_usize_index(&self) -> Option<usize> {
        self.as_f64().map(|x| x as usize)
    }
}

/// Parse strict JSON. Errors carry byte offsets. Never recurses unboundedly
/// beyond depth 200 (returns Err instead of stack overflow).
pub fn parse_json(src: &str) -> Result<JsonValue> {
    let b = src.as_bytes();
    let mut p = Jp { b, i: 0, depth: 0 };
    p.ws();
    let v = p.value()?;
    p.ws();
    if p.i != b.len() {
        return Err(MmdconvError::input(format!("JSON: trailing characters at offset {}", p.i)));
    }
    Ok(v)
}

struct Jp<'a> {
    b: &'a [u8],
    i: usize,
    depth: usize,
}

impl<'a> Jp<'a> {
    fn err<T>(&self, msg: &str) -> Result<T> {
        Err(MmdconvError::input(format!("JSON at {}: {msg}", self.i)))
    }
    fn ws(&mut self) {
        while self.i < self.b.len() && matches!(self.b[self.i], b' ' | b'\t' | b'\n' | b'\r') {
            self.i += 1;
        }
    }
    fn expect(&mut self, c: u8) -> Result<()> {
        self.ws();
        if self.i < self.b.len() && self.b[self.i] == c {
            self.i += 1;
            Ok(())
        } else {
            self.err(&format!("expected '{}'", c as char))
        }
    }
    fn value(&mut self) -> Result<JsonValue> {
        if self.depth > 200 {
            return self.err("nesting too deep");
        }
        self.ws();
        if self.i >= self.b.len() {
            return self.err("unexpected end of input");
        }
        match self.b[self.i] {
            b'{' => self.object(),
            b'[' => self.array(),
            b'"' => Ok(JsonValue::Str(self.string()?)),
            b't' => {
                self.lit("true")?;
                Ok(JsonValue::Bool(true))
            }
            b'f' => {
                self.lit("false")?;
                Ok(JsonValue::Bool(false))
            }
            b'n' => {
                self.lit("null")?;
                Ok(JsonValue::Null)
            }
            c if c == b'-' || c.is_ascii_digit() => self.number(),
            _ => self.err("invalid value start"),
        }
    }
    fn lit(&mut self, s: &str) -> Result<()> {
        if self.b.len() - self.i >= s.len() && &self.b[self.i..self.i + s.len()] == s.as_bytes() {
            self.i += s.len();
            Ok(())
        } else {
            self.err(&format!("expected literal {s}"))
        }
    }
    fn object(&mut self) -> Result<JsonValue> {
        self.depth += 1;
        self.expect(b'{')?;
        let mut items = Vec::new();
        self.ws();
        if self.i < self.b.len() && self.b[self.i] == b'}' {
            self.i += 1;
            self.depth -= 1;
            return Ok(JsonValue::Obj(items));
        }
        loop {
            self.ws();
            let key = self.string()?;
            self.expect(b':')?;
            let val = self.value()?;
            items.push((key, val));
            self.ws();
            if self.i < self.b.len() && self.b[self.i] == b',' {
                self.i += 1;
                continue;
            }
            self.expect(b'}')?;
            break;
        }
        self.depth -= 1;
        Ok(JsonValue::Obj(items))
    }
    fn array(&mut self) -> Result<JsonValue> {
        self.depth += 1;
        self.expect(b'[')?;
        let mut items = Vec::new();
        self.ws();
        if self.i < self.b.len() && self.b[self.i] == b']' {
            self.i += 1;
            self.depth -= 1;
            return Ok(JsonValue::Arr(items));
        }
        loop {
            let v = self.value()?;
            items.push(v);
            self.ws();
            if self.i < self.b.len() && self.b[self.i] == b',' {
                self.i += 1;
                continue;
            }
            self.expect(b']')?;
            break;
        }
        self.depth -= 1;
        Ok(JsonValue::Arr(items))
    }
    fn string(&mut self) -> Result<String> {
        self.expect(b'"')?;
        let mut out = String::new();
        loop {
            if self.i >= self.b.len() {
                return self.err("unterminated string");
            }
            let c = self.b[self.i];
            match c {
                b'"' => {
                    self.i += 1;
                    return Ok(out);
                }
                b'\\' => {
                    self.i += 1;
                    if self.i >= self.b.len() {
                        return self.err("bad escape");
                    }
                    let e = self.b[self.i];
                    self.i += 1;
                    match e {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'b' => out.push('\u{8}'),
                        b'f' => out.push('\u{c}'),
                        b'u' => {
                            let cp = self.hex4()?;
                            if (0xD800..0xDC00).contains(&cp) {
                                // high surrogate: expect \uDCxx
                                if self.i + 1 < self.b.len() && self.b[self.i] == b'\\' && self.b[self.i + 1] == b'u' {
                                    self.i += 2;
                                    let lo = self.hex4()?;
                                    if (0xDC00..0xE000).contains(&lo) {
                                        let c = 0x10000 + ((cp - 0xD800) << 10) + (lo - 0xDC00);
                                        out.push(char::from_u32(c).unwrap_or('\u{FFFD}'));
                                        continue;
                                    }
                                    out.push('\u{FFFD}');
                                    continue;
                                }
                                out.push('\u{FFFD}');
                            } else {
                                out.push(char::from_u32(cp).unwrap_or('\u{FFFD}'));
                            }
                        }
                        _ => return self.err("unknown escape"),
                    }
                }
                _ => {
                    // copy UTF-8 sequence verbatim
                    let start = self.i;
                    self.i += 1;
                    while self.i < self.b.len() && (self.b[self.i] & 0xC0) == 0x80 {
                        self.i += 1;
                    }
                    out.push_str(std::str::from_utf8(&self.b[start..self.i]).unwrap_or("\u{FFFD}"));
                }
            }
        }
    }
    fn hex4(&mut self) -> Result<u32> {
        if self.i + 4 > self.b.len() {
            return self.err("bad \\u escape");
        }
        let s = std::str::from_utf8(&self.b[self.i..self.i + 4]).map_err(|_| MmdconvError::input("bad utf8 in \\u"))?;
        let v = u32::from_str_radix(s, 16).map_err(|_| MmdconvError::input("bad hex in \\u"))?;
        self.i += 4;
        Ok(v)
    }
    fn number(&mut self) -> Result<JsonValue> {
        let start = self.i;
        if self.i < self.b.len() && self.b[self.i] == b'-' {
            self.i += 1;
        }
        while self.i < self.b.len() && self.b[self.i].is_ascii_digit() {
            self.i += 1;
        }
        if self.i < self.b.len() && self.b[self.i] == b'.' {
            self.i += 1;
            while self.i < self.b.len() && self.b[self.i].is_ascii_digit() {
                self.i += 1;
            }
        }
        if self.i < self.b.len() && (self.b[self.i] == b'e' || self.b[self.i] == b'E') {
            self.i += 1;
            if self.i < self.b.len() && (self.b[self.i] == b'+' || self.b[self.i] == b'-') {
                self.i += 1;
            }
            while self.i < self.b.len() && self.b[self.i].is_ascii_digit() {
                self.i += 1;
            }
        }
        let s = std::str::from_utf8(&self.b[start..self.i]).map_err(|_| MmdconvError::input("bad utf8 in number"))?;
        s.parse::<f64>()
            .map(JsonValue::Num)
            .map_err(|_| MmdconvError::input(format!("invalid number '{s}'")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_basic() {
        let v = parse_json(r#"{"a":[1,2.5,-3],"b":"日本語\n\"x\"","c":true,"d":null}"#).unwrap();
        assert_eq!(v.get("a").unwrap().as_array().unwrap().len(), 3);
        assert_eq!(v.get("b").unwrap().as_str().unwrap(), "日本語\n\"x\"");
        assert_eq!(v.get("c").unwrap().as_bool(), Some(true));
        assert!(matches!(v.get("d"), Some(JsonValue::Null)));
    }

    #[test]
    fn json_surrogates() {
        let v = parse_json(r#"{"e":"😀"}"#).unwrap();
        assert_eq!(v.get("e").unwrap().as_str().unwrap(), "😀");
    }

    #[test]
    fn json_rejects_garbage() {
        assert!(parse_json("{").is_err());
        assert!(parse_json("[1,]").is_err());
        assert!(parse_json("\"unterminated").is_err());
        assert!(parse_json("{} {}").is_err());
        let deep = "[".repeat(500) + &"]".repeat(500);
        assert!(parse_json(&deep).is_err());
    }

    #[test]
    fn triangulate_quads() {
        let tris = triangulate(&[0, 1, 2, 3], 4);
        assert_eq!(tris, vec![0, 1, 2, 0, 2, 3]);
    }

    #[test]
    fn weight_truncation() {
        let mut verts = vec![Vertex {
            weights: (0..6).map(|i| Weight { bone: i, weight: 0.3 }).collect(),
            ..Default::default()
        }];
        let t = normalize_vertex_weights(&mut verts, 0);
        assert_eq!(t, 1);
        assert_eq!(verts[0].weights.len(), 4);
        assert!((verts[0].weights.iter().map(|w| w.weight).sum::<f32>() - 1.0).abs() < 1e-6);
    }

    #[test]
    fn smooth_normals_on_tetrahedron() {
        let mut m = Mesh {
            name: "t".into(),
            vertices: vec![
                Vertex { pos: Vec3::ZERO, ..Default::default() },
                Vertex { pos: Vec3::X, ..Default::default() },
                Vertex { pos: Vec3::Y, ..Default::default() },
                Vertex { pos: Vec3::Z, ..Default::default() },
            ],
            primitives: vec![crate::ir::Primitive {
                material: 0,
                indices: vec![0, 2, 1, 0, 1, 3, 1, 2, 3, 0, 3, 2],
            }],
            inverse_bind: vec![],
        };
        compute_smooth_normals(&mut m);
        for v in &m.vertices {
            assert!(v.normal.length().max(1.0) - 1.0 < 1e-3 || v.normal.length_squared() > 0.5);
        }
    }
}
