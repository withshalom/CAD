//! Entity-specific data decoders.
//!
//! Written from knowledge of the spec, chapter 20 "Data section
//! AcDb:AcDbObjects" (per-type layouts). The fetch of the spec text stopped
//! before this chapter, so every layout here must be VERIFIED against the
//! spec PDF and a corpus of real files. Fields marked "uncertain" are the
//! ones most likely to need correction.

use super::streams::Streams;
use super::version::DwgVersion as V;
use crate::error::{Error, Result};
use crate::geom::{Vec2, Vec3};
use crate::model::*;

fn v3(a: [f64; 3]) -> Vec3 {
    Vec3::from_array(a)
}
fn v2(a: [f64; 2]) -> Vec2 {
    Vec2::from_array(a)
}

pub(crate) fn line(s: &mut Streams) -> Result<Line> {
    let (start, end) = if s.ver <= V::R14 {
        (v3(s.dat.bd3()?), v3(s.dat.bd3()?))
    } else {
        let z_zero = s.dat.b()?;
        let sx = s.dat.rd()?;
        let ex = s.dat.dd(sx)?;
        let sy = s.dat.rd()?;
        let ey = s.dat.dd(sy)?;
        let (sz, ez) = if z_zero {
            (0.0, 0.0)
        } else {
            let sz = s.dat.rd()?;
            (sz, s.dat.dd(sz)?)
        };
        (Vec3::new(sx, sy, sz), Vec3::new(ex, ey, ez))
    };
    let thickness = s.dat.bt(s.ver)?;
    let extrusion = v3(s.dat.be(s.ver)?);
    Ok(Line {
        start,
        end,
        thickness,
        extrusion,
    })
}

pub(crate) fn circle(s: &mut Streams) -> Result<Circle> {
    let center = v3(s.dat.bd3()?);
    let radius = s.dat.bd()?;
    let thickness = s.dat.bt(s.ver)?;
    let extrusion = v3(s.dat.be(s.ver)?);
    Ok(Circle {
        center,
        radius,
        thickness,
        extrusion,
    })
}

pub(crate) fn arc(s: &mut Streams) -> Result<Arc> {
    let c = circle(s)?;
    let start_angle = s.dat.bd()?;
    let end_angle = s.dat.bd()?;
    Ok(Arc {
        center: c.center,
        radius: c.radius,
        thickness: c.thickness,
        extrusion: c.extrusion,
        start_angle,
        end_angle,
    })
}

pub(crate) fn point(s: &mut Streams) -> Result<Point> {
    let position = v3(s.dat.bd3()?);
    let thickness = s.dat.bt(s.ver)?;
    let extrusion = v3(s.dat.be(s.ver)?);
    let x_axis_angle = s.dat.bd()?;
    Ok(Point {
        position,
        thickness,
        extrusion,
        x_axis_angle,
    })
}

pub(crate) fn ellipse(s: &mut Streams) -> Result<Ellipse> {
    Ok(Ellipse {
        center: v3(s.dat.bd3()?),
        major_axis: v3(s.dat.bd3()?),
        extrusion: v3(s.dat.bd3()?),
        axis_ratio: s.dat.bd()?,
        start_param: s.dat.bd()?,
        end_param: s.dat.bd()?,
    })
}

pub(crate) fn text(s: &mut Streams) -> Result<Text> {
    let mut t = Text {
        width_factor: 1.0,
        ..Default::default()
    };
    if s.ver <= V::R14 {
        t.elevation = s.dat.bd()?;
        t.insertion = v2(s.dat.rd2()?);
        t.alignment = v2(s.dat.rd2()?);
        t.extrusion = v3(s.dat.bd3()?);
        t.thickness = s.dat.bd()?;
        t.oblique = s.dat.bd()?;
        t.rotation = s.dat.bd()?;
        t.height = s.dat.bd()?;
        t.width_factor = s.dat.bd()?;
        t.value = s.tv()?;
        t.generation = s.dat.bs()?;
        t.h_align = s.dat.bs()?;
        t.v_align = s.dat.bs()?;
    } else {
        let flags = s.dat.rc()?;
        if flags & 0x01 == 0 {
            t.elevation = s.dat.rd()?;
        }
        t.insertion = v2(s.dat.rd2()?);
        t.alignment = if flags & 0x02 == 0 {
            let x = s.dat.dd(t.insertion.x)?;
            let y = s.dat.dd(t.insertion.y)?;
            Vec2::new(x, y)
        } else {
            t.insertion
        };
        t.extrusion = v3(s.dat.be(s.ver)?);
        t.thickness = s.dat.bt(s.ver)?;
        if flags & 0x04 == 0 {
            t.oblique = s.dat.rd()?;
        }
        if flags & 0x08 == 0 {
            t.rotation = s.dat.rd()?;
        }
        t.height = s.dat.rd()?;
        if flags & 0x10 == 0 {
            t.width_factor = s.dat.rd()?;
        }
        t.value = s.tv()?;
        if flags & 0x20 == 0 {
            t.generation = s.dat.bs()?;
        }
        if flags & 0x40 == 0 {
            t.h_align = s.dat.bs()?;
        }
        if flags & 0x80 == 0 {
            t.v_align = s.dat.bs()?;
        }
    }
    Ok(t)
}

/// ATTRIB (definition = false) and ATTDEF (definition = true).
pub(crate) fn attribute(s: &mut Streams, definition: bool) -> Result<Attribute> {
    let text = text(s)?;
    if s.ver >= V::R2010 {
        let _version = s.dat.rc()?;
    }
    if s.ver >= V::R2018 {
        // Uncertain: attribute type (1 single line, 2/4 multi-line).
        let kind = s.dat.rc()?;
        if kind > 1 {
            return Err(Error::unsupported(
                "multi-line attribute (R2018) not decoded yet",
            ));
        }
    }
    let tag = s.tv()?;
    let field_length = s.dat.bs()?;
    let flags = s.dat.rc()?;
    if s.ver >= V::R2007 {
        let _lock_position = s.dat.b()?;
    }
    let prompt = if definition {
        if s.ver >= V::R2010 {
            let _version = s.dat.rc()?; // uncertain
        }
        s.tv()?
    } else {
        String::new()
    };
    Ok(Attribute {
        text,
        tag,
        prompt,
        flags,
        field_length,
        is_definition: definition,
    })
}

/// INSERT and MINSERT. Returns the insert and, for R2004+, the number of
/// owned attribute handles that follow in the handle stream.
pub(crate) fn insert(s: &mut Streams, minsert: bool) -> Result<(Insert, u32)> {
    let insertion = v3(s.dat.bd3()?);
    let scale = if s.ver <= V::R14 {
        v3(s.dat.bd3()?)
    } else {
        match s.dat.bb()? {
            0 => {
                let x = s.dat.rd()?;
                let y = s.dat.dd(x)?;
                let z = s.dat.dd(x)?;
                Vec3::new(x, y, z)
            }
            1 => {
                let y = s.dat.dd(1.0)?;
                let z = s.dat.dd(1.0)?;
                Vec3::new(1.0, y, z)
            }
            2 => {
                let x = s.dat.rd()?;
                Vec3::new(x, x, x)
            }
            _ => Vec3::new(1.0, 1.0, 1.0),
        }
    };
    let rotation = s.dat.bd()?;
    let extrusion = v3(s.dat.bd3()?);
    let has_attribs = s.dat.b()?;
    let owned = if s.ver >= V::R2004 && has_attribs {
        s.dat.bl()?
    } else {
        0
    };
    let array = if minsert {
        let cols = s.dat.bs()?;
        let rows = s.dat.bs()?;
        let col_spacing = s.dat.bd()?;
        let row_spacing = s.dat.bd()?;
        Some((cols, rows, col_spacing, row_spacing))
    } else {
        None
    };
    Ok((
        Insert {
            insertion,
            scale,
            rotation,
            extrusion,
            has_attribs,
            array,
            ..Default::default()
        },
        owned,
    ))
}

pub(crate) fn vertex_2d(s: &mut Streams) -> Result<Vertex> {
    let flags = s.dat.rc()?;
    let point = v3(s.dat.bd3()?);
    let sw = s.dat.bd()?;
    let (start_width, end_width) = if sw < 0.0 {
        (-sw, -sw)
    } else {
        (sw, s.dat.bd()?)
    };
    let bulge = s.dat.bd()?;
    let id = if s.ver >= V::R2010 { s.dat.bl()? } else { 0 };
    let tangent_dir = s.dat.bd()?;
    Ok(Vertex {
        kind: VertexKind::Vertex2d,
        flags,
        point,
        start_width,
        end_width,
        bulge,
        tangent_dir,
        id,
        face: [0; 4],
    })
}

pub(crate) fn vertex_3d(s: &mut Streams, kind: VertexKind) -> Result<Vertex> {
    let flags = s.dat.rc()?;
    let point = v3(s.dat.bd3()?);
    Ok(Vertex {
        kind,
        flags,
        point,
        ..Default::default()
    })
}

pub(crate) fn vertex_pface_face(s: &mut Streams) -> Result<Vertex> {
    let mut face = [0i16; 4];
    for f in &mut face {
        *f = s.dat.bs()? as i16;
    }
    Ok(Vertex {
        kind: VertexKind::PfaceFace,
        face,
        ..Default::default()
    })
}

/// POLYLINE (2D). Returns the owned-object count (R2004+).
pub(crate) fn polyline_2d(s: &mut Streams) -> Result<(Polyline, u32)> {
    let flags = s.dat.bs()?;
    let curve_type = s.dat.bs()?;
    let start_width = s.dat.bd()?;
    let end_width = s.dat.bd()?;
    let thickness = s.dat.bt(s.ver)?;
    let elevation = s.dat.bd()?;
    let extrusion = v3(s.dat.be(s.ver)?);
    let owned = if s.ver >= V::R2004 { s.dat.bl()? } else { 0 };
    Ok((
        Polyline {
            kind: PolylineKind::Polyline2d,
            flags,
            curve_type,
            start_width,
            end_width,
            thickness,
            elevation,
            extrusion,
            ..Default::default()
        },
        owned,
    ))
}

/// POLYLINE (3D).
pub(crate) fn polyline_3d(s: &mut Streams) -> Result<(Polyline, u32)> {
    let curve_flags = s.dat.rc()?;
    let closed_flags = s.dat.rc()?;
    let owned = if s.ver >= V::R2004 { s.dat.bl()? } else { 0 };
    Ok((
        Polyline {
            kind: PolylineKind::Polyline3d,
            flags: (closed_flags as u16 & 1) | 8,
            curve_type: curve_flags as u16,
            extrusion: Vec3::Z,
            ..Default::default()
        },
        owned,
    ))
}

/// POLYLINE (PFACE).
pub(crate) fn polyline_pface(s: &mut Streams) -> Result<(Polyline, u32)> {
    let _num_verts = s.dat.bs()?;
    let _num_faces = s.dat.bs()?;
    let owned = if s.ver >= V::R2004 { s.dat.bl()? } else { 0 };
    Ok((
        Polyline {
            kind: PolylineKind::PolyfaceMesh,
            flags: 64,
            extrusion: Vec3::Z,
            ..Default::default()
        },
        owned,
    ))
}

/// POLYLINE (MESH).
pub(crate) fn polyline_mesh(s: &mut Streams) -> Result<(Polyline, u32)> {
    let flags = s.dat.bs()?;
    let curve_type = s.dat.bs()?;
    let _m = s.dat.bs()?;
    let _n = s.dat.bs()?;
    let _m_density = s.dat.bs()?;
    let _n_density = s.dat.bs()?;
    let owned = if s.ver >= V::R2004 { s.dat.bl()? } else { 0 };
    Ok((
        Polyline {
            kind: PolylineKind::PolygonMesh,
            flags: flags | 16,
            curve_type,
            extrusion: Vec3::Z,
            ..Default::default()
        },
        owned,
    ))
}

pub(crate) fn lwpolyline(s: &mut Streams) -> Result<LwPolyline> {
    let flags = s.dat.bs()?;
    let mut p = LwPolyline {
        flags,
        extrusion: Vec3::Z,
        ..Default::default()
    };
    if flags & 4 != 0 {
        p.const_width = s.dat.bd()?;
    }
    if flags & 8 != 0 {
        p.elevation = s.dat.bd()?;
    }
    if flags & 2 != 0 {
        p.thickness = s.dat.bd()?;
    }
    if flags & 1 != 0 {
        p.extrusion = v3(s.dat.bd3()?);
    }
    let n_points = s.dat.bl()?;
    let n_bulges = if flags & 16 != 0 { s.dat.bl()? } else { 0 };
    let n_ids = if s.ver >= V::R2010 && flags & 1024 != 0 {
        s.dat.bl()?
    } else {
        0
    };
    let n_widths = if flags & 32 != 0 { s.dat.bl()? } else { 0 };

    let n_points = s.check_count(n_points, 4, "LWPOLYLINE point")?;
    let n_bulges = s.check_count(n_bulges, 2, "LWPOLYLINE bulge")?;
    let n_ids = s.check_count(n_ids, 2, "LWPOLYLINE vertex id")?;
    let n_widths = s.check_count(n_widths, 4, "LWPOLYLINE width")?;

    p.points.reserve(n_points);
    if s.ver <= V::R14 {
        for _ in 0..n_points {
            p.points.push(v2(s.dat.rd2()?));
        }
    } else if n_points > 0 {
        let mut prev = v2(s.dat.rd2()?);
        p.points.push(prev);
        for _ in 1..n_points {
            let x = s.dat.dd(prev.x)?;
            let y = s.dat.dd(prev.y)?;
            prev = Vec2::new(x, y);
            p.points.push(prev);
        }
    }
    for _ in 0..n_bulges {
        p.bulges.push(s.dat.bd()?);
    }
    for _ in 0..n_ids {
        p.vertex_ids.push(s.dat.bl()?);
    }
    for _ in 0..n_widths {
        let a = s.dat.bd()?;
        let b = s.dat.bd()?;
        p.widths.push((a, b));
    }
    Ok(p)
}

/// SOLID and TRACE share a layout.
pub(crate) fn solid(s: &mut Streams, is_trace: bool) -> Result<Solid> {
    let thickness = s.dat.bt(s.ver)?;
    let elevation = s.dat.bd()?;
    let mut corners = [Vec2::ZERO; 4];
    for c in &mut corners {
        *c = v2(s.dat.rd2()?);
    }
    let extrusion = v3(s.dat.be(s.ver)?);
    Ok(Solid {
        thickness,
        elevation,
        corners,
        extrusion,
        is_trace,
    })
}

pub(crate) fn face3d(s: &mut Streams) -> Result<Face3d> {
    let mut f = Face3d::default();
    if s.ver <= V::R14 {
        for c in &mut f.corners {
            *c = v3(s.dat.bd3()?);
        }
        f.invisible_edges = s.dat.bs()?;
    } else {
        let no_flags = s.dat.b()?;
        let z_zero = s.dat.b()?;
        let x = s.dat.rd()?;
        let y = s.dat.rd()?;
        let z = if z_zero { 0.0 } else { s.dat.rd()? };
        f.corners[0] = Vec3::new(x, y, z);
        for i in 1..4 {
            let prev = f.corners[i - 1];
            let x = s.dat.dd(prev.x)?;
            let y = s.dat.dd(prev.y)?;
            let z = s.dat.dd(prev.z)?;
            f.corners[i] = Vec3::new(x, y, z);
        }
        if !no_flags {
            f.invisible_edges = s.dat.bs()?;
        }
    }
    Ok(f)
}

pub(crate) fn spline(s: &mut Streams) -> Result<Spline> {
    let mut sp = Spline::default();
    let mut scenario = s.dat.bl()?;
    if s.ver >= V::R2013 {
        // Uncertain: R2013 adds spline flags and knot parameterisation.
        let flags1 = s.dat.bl()?;
        let knot_param = s.dat.bl()?;
        if flags1 & 1 != 0 {
            scenario = 2;
        }
        if knot_param == 15 {
            scenario = 1;
        }
    }
    sp.degree = s.dat.bl()?;
    let mut n_knots = 0;
    let mut n_ctrl = 0;
    let mut n_fit = 0;
    let mut weighted = false;
    match scenario {
        2 => {
            sp.fit_tolerance = s.dat.bd()?;
            sp.start_tangent = v3(s.dat.bd3()?);
            sp.end_tangent = v3(s.dat.bd3()?);
            n_fit = s.dat.bl()?;
        }
        1 => {
            sp.rational = s.dat.b()?;
            sp.closed = s.dat.b()?;
            sp.periodic = s.dat.b()?;
            let _knot_tol = s.dat.bd()?;
            let _ctrl_tol = s.dat.bd()?;
            n_knots = s.dat.bl()?;
            n_ctrl = s.dat.bl()?;
            weighted = s.dat.b()?;
        }
        other => return Err(Error::format(format!("unknown spline scenario {other}"))),
    }
    let n_knots = s.check_count(n_knots, 2, "SPLINE knot")?;
    let n_ctrl = s.check_count(n_ctrl, 6, "SPLINE control point")?;
    let n_fit = s.check_count(n_fit, 6, "SPLINE fit point")?;
    for _ in 0..n_knots {
        sp.knots.push(s.dat.bd()?);
    }
    for _ in 0..n_ctrl {
        sp.control_points.push(v3(s.dat.bd3()?));
        if weighted {
            sp.weights.push(s.dat.bd()?);
        }
    }
    for _ in 0..n_fit {
        sp.fit_points.push(v3(s.dat.bd3()?));
    }
    Ok(sp)
}

pub(crate) fn mtext(s: &mut Streams) -> Result<MText> {
    let mut m = MText {
        insertion: v3(s.dat.bd3()?),
        extrusion: v3(s.dat.bd3()?),
        x_axis: v3(s.dat.bd3()?),
        linespacing_factor: 1.0,
        ..Default::default()
    };
    if s.ver >= V::R2007 {
        m.rect_height = s.dat.bd()?; // uncertain
    }
    m.rect_width = s.dat.bd()?;
    m.text_height = s.dat.bd()?;
    m.attachment = s.dat.bs()?;
    m.drawing_dir = s.dat.bs()?;
    let _ext_height = s.dat.bd()?;
    let _ext_width = s.dat.bd()?;
    m.contents = s.tv()?;
    if s.ver >= V::R2000 {
        let _linespacing_style = s.dat.bs()?;
        m.linespacing_factor = s.dat.bd()?;
        let _unknown = s.dat.b()?;
    }
    // R2004+ background fill and R2018 column data follow; not needed for
    // display, and the handle stream is read from its own position.
    Ok(m)
}

pub(crate) fn dimension(s: &mut Streams, code: u16) -> Result<Dimension> {
    let mut d = Dimension::default();
    if s.ver >= V::R2010 {
        let _version = s.dat.rc()?;
    }
    d.extrusion = v3(s.dat.bd3()?);
    d.text_midpoint = v2(s.dat.rd2()?);
    d.elevation = s.dat.bd()?;
    d.flags = s.dat.rc()?;
    d.user_text = s.tv()?;
    d.text_rotation = s.dat.bd()?;
    d.horizontal_direction = s.dat.bd()?;
    d.insert_scale = v3(s.dat.bd3()?);
    d.insert_rotation = s.dat.bd()?;
    if s.ver >= V::R2000 {
        d.attachment = s.dat.bs()?;
        let _linespacing_style = s.dat.bs()?;
        let _linespacing_factor = s.dat.bd()?;
        d.measurement = s.dat.bd()?;
    }
    if s.ver >= V::R2007 {
        let _unknown = s.dat.b()?;
        let _flip_arrow1 = s.dat.b()?;
        let _flip_arrow2 = s.dat.b()?;
    }
    d.clone_insertion = v2(s.dat.rd2()?);
    match code {
        0x14 => {
            d.kind = DimensionKind::Ordinate;
            d.pt10 = v3(s.dat.bd3()?);
            d.pt13 = v3(s.dat.bd3()?);
            d.pt14 = v3(s.dat.bd3()?);
            let _flags2 = s.dat.rc()?;
        }
        0x15 => {
            d.kind = DimensionKind::Linear;
            d.pt13 = v3(s.dat.bd3()?);
            d.pt14 = v3(s.dat.bd3()?);
            d.pt10 = v3(s.dat.bd3()?);
            d.ext_line_rotation = s.dat.bd()?;
            d.dim_rotation = s.dat.bd()?;
        }
        0x16 => {
            d.kind = DimensionKind::Aligned;
            d.pt13 = v3(s.dat.bd3()?);
            d.pt14 = v3(s.dat.bd3()?);
            d.pt10 = v3(s.dat.bd3()?);
            d.ext_line_rotation = s.dat.bd()?;
        }
        0x17 => {
            d.kind = DimensionKind::Angular3Pt;
            d.pt10 = v3(s.dat.bd3()?);
            d.pt13 = v3(s.dat.bd3()?);
            d.pt14 = v3(s.dat.bd3()?);
            d.pt15 = v3(s.dat.bd3()?);
        }
        0x18 => {
            d.kind = DimensionKind::Angular2Line;
            d.pt16 = v2(s.dat.rd2()?);
            d.pt13 = v3(s.dat.bd3()?);
            d.pt14 = v3(s.dat.bd3()?);
            d.pt15 = v3(s.dat.bd3()?);
            d.pt10 = v3(s.dat.bd3()?);
        }
        0x19 | 0x1A => {
            d.kind = if code == 0x19 {
                DimensionKind::Radius
            } else {
                DimensionKind::Diameter
            };
            d.pt10 = v3(s.dat.bd3()?);
            d.pt15 = v3(s.dat.bd3()?);
            d.leader_length = s.dat.bd()?;
        }
        _ => return Err(Error::format(format!("not a dimension type {code:#x}"))),
    }
    Ok(d)
}

/// HATCH. Returns the hatch and the number of boundary-object handles in
/// the handle stream.
pub(crate) fn hatch(s: &mut Streams) -> Result<(Hatch, u32)> {
    let mut h = Hatch::default();
    if s.ver >= V::R2004 {
        h.is_gradient = s.dat.bl()? != 0;
        let _reserved = s.dat.bl()?;
        let _angle = s.dat.bd()?;
        let _shift = s.dat.bd()?;
        let _single_color = s.dat.bl()?;
        let _tint = s.dat.bd()?;
        let n_colors = s.dat.bl()?;
        let n_colors = s.check_count(n_colors, 20, "gradient color")?;
        for _ in 0..n_colors {
            let _unknown_double = s.dat.bd()?;
            let _unknown_short = s.dat.bs()?;
            let _rgb = s.dat.bl()?;
            let _ignored = s.dat.rc()?;
        }
        let _gradient_name = s.tv()?;
    }
    h.elevation = s.dat.bd()?;
    h.extrusion = v3(s.dat.bd3()?);
    h.pattern_name = s.tv()?;
    h.solid_fill = s.dat.b()?;
    h.associative = s.dat.b()?;
    let n_paths = s.dat.bl()?;
    let n_paths = s.check_count(n_paths, 8, "hatch path")?;
    let mut handles = 0u32;
    let mut derived = false;
    for _ in 0..n_paths {
        let flags = s.dat.bl()?;
        if flags & 4 != 0 {
            derived = true;
        }
        if flags & 2 == 0 {
            let n_edges = s.dat.bl()?;
            let n_edges = s.check_count(n_edges, 8, "hatch edge")?;
            let mut edges = Vec::with_capacity(n_edges);
            for _ in 0..n_edges {
                let kind = s.dat.rc()?;
                let edge = match kind {
                    1 => HatchEdge::Line {
                        a: v2(s.dat.rd2()?),
                        b: v2(s.dat.rd2()?),
                    },
                    2 => HatchEdge::Arc {
                        center: v2(s.dat.rd2()?),
                        radius: s.dat.bd()?,
                        start: s.dat.bd()?,
                        end: s.dat.bd()?,
                        ccw: s.dat.b()?,
                    },
                    3 => HatchEdge::Ellipse {
                        center: v2(s.dat.rd2()?),
                        major: v2(s.dat.rd2()?),
                        ratio: s.dat.bd()?,
                        start: s.dat.bd()?,
                        end: s.dat.bd()?,
                        ccw: s.dat.b()?,
                    },
                    4 => {
                        let degree = s.dat.bl()?;
                        let rational = s.dat.b()?;
                        let _periodic = s.dat.b()?;
                        let n_knots = s.dat.bl()?;
                        let n_ctrl = s.dat.bl()?;
                        let n_knots = s.check_count(n_knots, 2, "hatch spline knot")?;
                        let n_ctrl = s.check_count(n_ctrl, 128, "hatch spline control point")?;
                        let mut knots = Vec::with_capacity(n_knots);
                        for _ in 0..n_knots {
                            knots.push(s.dat.bd()?);
                        }
                        let mut control_points = Vec::with_capacity(n_ctrl);
                        let mut weights = Vec::new();
                        for _ in 0..n_ctrl {
                            control_points.push(v2(s.dat.rd2()?));
                            if rational {
                                weights.push(s.dat.bd()?);
                            }
                        }
                        if s.ver >= V::R2010 {
                            let n_fit = s.dat.bl()?;
                            let n_fit = s.check_count(n_fit, 128, "hatch spline fit point")?;
                            if n_fit > 0 {
                                for _ in 0..n_fit {
                                    let _fit = s.dat.rd2()?;
                                }
                                let _start_tangent = s.dat.rd2()?;
                                let _end_tangent = s.dat.rd2()?;
                            }
                        }
                        HatchEdge::Spline {
                            degree,
                            knots,
                            control_points,
                            weights,
                        }
                    }
                    other => return Err(Error::format(format!("unknown hatch edge type {other}"))),
                };
                edges.push(edge);
            }
            h.paths.push(HatchPath::Edges { flags, edges });
        } else {
            let has_bulges = s.dat.b()?;
            let closed = s.dat.b()?;
            let n = s.dat.bl()?;
            let n = s.check_count(n, 128, "hatch polyline vertex")?;
            let mut vertices = Vec::with_capacity(n);
            for _ in 0..n {
                let p = v2(s.dat.rd2()?);
                let b = if has_bulges { s.dat.bd()? } else { 0.0 };
                vertices.push((p, b));
            }
            h.paths.push(HatchPath::Polyline {
                flags,
                closed,
                vertices,
            });
        }
        handles = handles.saturating_add(s.dat.bl()?);
    }
    h.style = s.dat.bs()?;
    h.pattern_type = s.dat.bs()?;
    if !h.solid_fill {
        h.pattern_angle = s.dat.bd()?;
        h.pattern_scale = s.dat.bd()?;
        h.pattern_double = s.dat.b()?;
        let n_lines = s.dat.bs()?;
        let n_lines = s.check_count(n_lines as u32, 10, "hatch pattern line")?;
        for _ in 0..n_lines {
            let angle = s.dat.bd()?;
            let base = v2(s.dat.bd2()?);
            let offset = v2(s.dat.bd2()?);
            let n_dashes = s.dat.bs()?;
            let n_dashes = s.check_count(n_dashes as u32, 2, "hatch dash")?;
            let mut dashes = Vec::with_capacity(n_dashes);
            for _ in 0..n_dashes {
                dashes.push(s.dat.bd()?);
            }
            h.pattern_lines.push(HatchPatternLine {
                angle,
                base,
                offset,
                dashes,
            });
        }
    }
    if derived {
        let _pixel_size = s.dat.bd()?;
    }
    let n_seeds = s.dat.bl()?;
    let n_seeds = s.check_count(n_seeds, 128, "hatch seed point")?;
    for _ in 0..n_seeds {
        h.seed_points.push(v2(s.dat.rd2()?));
    }
    Ok((h, handles))
}

/// RAY and XLINE (§20.4.42, §20.4.43): a point and a direction.
pub(crate) fn construction_line(s: &mut Streams) -> Result<ConstructionLine> {
    Ok(ConstructionLine {
        base: v3(s.dat.bd3()?),
        direction: v3(s.dat.bd3()?),
    })
}

/// LEADER (§20.4.47), up to its handles.
pub(crate) fn leader(s: &mut Streams) -> Result<Leader> {
    let unknown_bit = s.dat.b()?;
    let annotation_type = s.dat.bs()?;
    let path_type = s.dat.bs()?;
    let n = s.dat.bl()?;
    let n = s.check_count(n, 6, "leader vertex")?;
    let mut vertices = Vec::with_capacity(n);
    for _ in 0..n {
        vertices.push(v3(s.dat.bd3()?));
    }
    let origin = v3(s.dat.bd3()?);
    let extrusion = v3(s.dat.bd3()?);
    let x_direction = v3(s.dat.bd3()?);
    let block_offset = v3(s.dat.bd3()?);
    let annotation_offset = if s.ver >= V::R14 { v3(s.dat.bd3()?) } else { Vec3::ZERO };
    if s.ver <= V::R14 {
        let _dimgap = s.dat.bd()?;
    }
    let text_height = s.dat.bd()?;
    let text_width = s.dat.bd()?;
    let hookline_on_x = s.dat.b()?;
    let arrowhead = s.dat.b()?;
    let mut unknown = (unknown_bit, 0u16, false, false);
    if s.ver <= V::R14 {
        let _arrowhead_type = s.dat.bs()?;
        let _dimasz = s.dat.bd()?;
        let _ = (s.dat.b()?, s.dat.b()?, s.dat.bs()?, s.dat.bs()?, s.dat.b()?, s.dat.b()?);
    } else {
        unknown.1 = s.dat.bs()?;
        unknown.2 = s.dat.b()?;
        unknown.3 = s.dat.b()?;
    }
    // DWG keeps no hook line flag: an MTEXT leader whose last segment is
    // not along the horizontal direction ends in one.
    let hookline = annotation_type == 0
        && vertices.len() >= 2
        && {
            let d = vertices[vertices.len() - 1] - vertices[vertices.len() - 2];
            let x = x_direction.normalized();
            let along = d.dot(x).abs();
            along < d.length() * (1.0 - 1e-9)
        };
    Ok(Leader {
        vertices,
        arrowhead,
        spline: path_type == 1,
        annotation_type,
        hookline_on_x,
        hookline,
        text_height,
        text_width,
        origin,
        extrusion,
        x_direction,
        block_offset,
        annotation_offset,
        annotation: Handle::NULL,
        dimstyle: Handle::NULL,
        unknown,
    })
}

/// VIEWPORT (§20.4.38), up to its handles; with the number of frozen
/// layer handles to read.
pub(crate) fn viewport(s: &mut Streams) -> Result<(PaperViewport, u32)> {
    let center = v3(s.dat.bd3()?);
    let width = s.dat.bd()?;
    let height = s.dat.bd()?;
    let mut v = PaperViewport::new(center.xy(), width, height, Vec2::ZERO, 1.0, 0);
    v.center = center;
    v.view_height = height;
    let mut frozen = 0;
    if s.ver >= V::R2000 {
        v.view_target = v3(s.dat.bd3()?);
        v.view_direction = v3(s.dat.bd3()?);
        v.twist = s.dat.bd()?;
        v.view_height = s.dat.bd()?;
        v.lens = s.dat.bd()?;
        v.front_clip = s.dat.bd()?;
        v.back_clip = s.dat.bd()?;
        v.snap_angle = s.dat.bd()?;
        v.view_center = v2(s.dat.rd2()?);
        v.snap_base = v2(s.dat.rd2()?);
        v.snap_spacing = v2(s.dat.rd2()?);
        v.grid_spacing = v2(s.dat.rd2()?);
        v.circle_zoom = s.dat.bs()?;
        if s.ver >= V::R2007 {
            v.grid_major = s.dat.bs()?;
        }
        frozen = s.dat.bl()?;
        v.status = s.dat.bl()?;
        v.style_sheet = s.tv()?;
        v.render_mode = s.dat.rc()?;
        v.ucs_at_origin = s.dat.b()?;
        v.ucs_per_viewport = s.dat.b()?;
        v.ucs_origin = v3(s.dat.bd3()?);
        v.ucs_x = v3(s.dat.bd3()?);
        v.ucs_y = v3(s.dat.bd3()?);
        v.ucs_elevation = s.dat.bd()?;
        v.ucs_ortho_type = s.dat.bs()?;
        if s.ver >= V::R2004 {
            v.shade_plot_mode = s.dat.bs()?;
        }
        if s.ver >= V::R2007 {
            v.default_lights = s.dat.b()?;
            v.lighting_type = s.dat.rc()?;
            v.brightness = s.dat.bd()?;
            v.contrast = s.dat.bd()?;
            v.ambient = s.cmc()?.0;
        }
    }
    Ok((v, frozen))
}
