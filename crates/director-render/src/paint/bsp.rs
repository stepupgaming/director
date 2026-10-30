//! Binary space partitioning for preserve-3d planes.
//!
//! Planes are convex polygons in the sorting parent's space. The tree splits
//! every polygon that crosses another's plane, then walks back to front from
//! the eye, so drawing the fragments in order resolves occlusion and
//! intersections exactly. Coplanar polygons keep their input order, so an
//! equal-depth tie still follows z-index and source order.

/// Distance under which a point counts as lying on a plane, in CSS pixels.
const ON_PLANE: f32 = 1.0e-2;

pub(super) type Point3 = [f32; 3];

/// Where the eye sits: a point for a perspective projection, or a direction
/// toward the viewer for an orthographic one.
#[derive(Clone, Copy)]
pub(super) enum Eye {
    Point(Point3),
    Direction(Point3),
}

/// A piece of input polygon `plane`. `split[i]` is true when the edge from
/// vertex `i` to the next was cut by another plane.
pub(super) struct Fragment {
    pub plane: usize,
    pub points: Vec<Point3>,
    pub split: Vec<bool>,
}

struct Plane {
    normal: Point3,
    offset: f32,
}

impl Plane {
    /// Newell's method keeps a stable normal for any convex polygon.
    fn of(points: &[Point3]) -> Option<Plane> {
        let mut normal = [0.0f32; 3];
        let mut center = [0.0f32; 3];
        for (i, a) in points.iter().enumerate() {
            let b = points[(i + 1) % points.len()];
            normal[0] += (a[1] - b[1]) * (a[2] + b[2]);
            normal[1] += (a[2] - b[2]) * (a[0] + b[0]);
            normal[2] += (a[0] - b[0]) * (a[1] + b[1]);
            for axis in 0..3 {
                center[axis] += a[axis] / points.len() as f32;
            }
        }
        let length = dot(normal, normal).sqrt();
        if !length.is_finite() || length < 1.0e-6 {
            return None;
        }
        let normal = normal.map(|value| value / length);
        Some(Plane { normal, offset: -dot(normal, center) })
    }

    fn distance(&self, point: Point3) -> f32 {
        dot(self.normal, point) + self.offset
    }

    fn faces(&self, eye: Eye) -> bool {
        match eye {
            Eye::Point(point) => self.distance(point) > 0.0,
            Eye::Direction(direction) => dot(self.normal, direction) > 0.0,
        }
    }
}

fn dot(a: Point3, b: Point3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// Fragments of `polygons` in back-to-front order seen from `eye`. `None`
/// when a polygon is degenerate and has no plane.
pub(super) fn back_to_front(polygons: &[Vec<Point3>], eye: Eye) -> Option<Vec<Fragment>> {
    let fragments = polygons
        .iter()
        .enumerate()
        .map(|(plane, points)| Fragment { plane, points: points.clone(), split: vec![false; points.len()] })
        .collect();
    let mut out = Vec::new();
    walk(fragments, eye, &mut out, 0)?;
    Some(out)
}

fn walk(mut fragments: Vec<Fragment>, eye: Eye, out: &mut Vec<Fragment>, depth: usize) -> Option<()> {
    if fragments.is_empty() {
        return Some(());
    }
    if depth > 256 {
        return None;
    }
    let splitter = fragments.remove(0);
    let plane = Plane::of(&splitter.points)?;
    let mut coplanar = vec![splitter];
    let (mut front, mut back) = (Vec::new(), Vec::new());
    for fragment in fragments {
        let distances: Vec<f32> = fragment.points.iter().map(|&point| plane.distance(point)).collect();
        let above = distances.iter().any(|&d| d > ON_PLANE);
        let below = distances.iter().any(|&d| d < -ON_PLANE);
        match (above, below) {
            (false, false) => coplanar.push(fragment),
            (true, false) => front.push(fragment),
            (false, true) => back.push(fragment),
            (true, true) => {
                let (ahead, behind) = split(&fragment, &distances);
                front.extend(ahead);
                back.extend(behind);
            }
        }
    }
    let (first, last) = if plane.faces(eye) { (back, front) } else { (front, back) };
    walk(first, eye, out, depth + 1)?;
    out.extend(coplanar);
    walk(last, eye, out, depth + 1)
}

/// Cuts a convex polygon along the plane its `distances` were measured to.
fn split(fragment: &Fragment, distances: &[f32]) -> (Option<Fragment>, Option<Fragment>) {
    let side = |d: f32| if d > ON_PLANE { 1 } else if d < -ON_PLANE { -1 } else { 0 };
    let count = fragment.points.len();
    let (mut front, mut back) = (Vec::new(), Vec::new());
    for i in 0..count {
        let next = (i + 1) % count;
        let (a, b) = (fragment.points[i], fragment.points[next]);
        let (sa, sb) = (side(distances[i]), side(distances[next]));
        let edge = fragment.split[i];
        if sa >= 0 {
            // A vertex on the plane starts the cut edge when its polygon leaves this side.
            front.push((a, if sa == 0 && sb < 0 { true } else { edge }));
        }
        if sa <= 0 {
            back.push((a, if sa == 0 && sb > 0 { true } else { edge }));
        }
        if sa * sb < 0 {
            let t = distances[i] / (distances[i] - distances[next]);
            let point = [0, 1, 2].map(|axis| a[axis] + (b[axis] - a[axis]) * t);
            if sa > 0 {
                front.push((point, true));
                back.push((point, edge));
            } else {
                back.push((point, true));
                front.push((point, edge));
            }
        }
    }
    let piece = |vertices: Vec<(Point3, bool)>| {
        (vertices.len() >= 3).then(|| Fragment {
            plane: fragment.plane,
            points: vertices.iter().map(|vertex| vertex.0).collect(),
            split: vertices.iter().map(|vertex| vertex.1).collect(),
        })
    };
    (piece(front), piece(back))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quad(z0: f32, z1: f32) -> Vec<Point3> {
        vec![[0.0, 0.0, z0], [100.0, 0.0, z1], [100.0, 100.0, z1], [0.0, 100.0, z0]]
    }

    #[test]
    fn parallel_planes_draw_far_to_near() {
        let eye = Eye::Point([50.0, 50.0, 1000.0]);
        let order: Vec<_> = back_to_front(&[quad(10.0, 10.0), quad(-10.0, -10.0)], eye).unwrap().iter().map(|f| f.plane).collect();
        assert_eq!(order, [1, 0]);
    }

    #[test]
    fn crossing_planes_split_once() {
        let eye = Eye::Point([50.0, 50.0, 1000.0]);
        let fragments = back_to_front(&[quad(0.0, 0.0), quad(-50.0, 50.0)], eye).unwrap();
        assert_eq!(fragments.len(), 3);
        // The tilted plane's far half draws first, then the flat plane, then its near half.
        assert_eq!(fragments.iter().map(|f| f.plane).collect::<Vec<_>>(), [1, 0, 1]);
        assert!(fragments[0].split.iter().any(|&cut| cut));
        assert!(fragments[0].points.iter().all(|p| p[0] <= 50.0 + 1.0e-3));
    }

    #[test]
    fn coplanar_planes_keep_input_order() {
        let eye = Eye::Direction([0.0, 0.0, 1.0]);
        let order: Vec<_> = back_to_front(&[quad(5.0, 5.0), quad(5.0, 5.0)], eye).unwrap().iter().map(|f| f.plane).collect();
        assert_eq!(order, [0, 1]);
    }
}
