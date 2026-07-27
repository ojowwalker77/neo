/* Movement transitions between two ordered mathsets.
 *
 * Motion only has an identity when row i still means row i. This deliberately
 * accepts a narrower contract than arbitrary interpolation. Plain transitions
 * allow only x/y changes. Rigid transitions require a descriptor whose
 * membership, pivot, translation, and angle reproduce endpoint B, then
 * evaluate centres along arcs and rotate the primitive axes. Neither path can
 * turn unrelated fits into an appearance cross-fade.
 */
use crate::format::{MathSet, linear_to_srgb, srgb_to_linear};
use crate::motion::{MotionGroup, MotionSet, apply_rigid};

fn matching_endpoints(from: &MathSet, to: &MathSet) -> Result<(), String> {
    if from.mathset != to.mathset
        || from.canvas != to.canvas
        || from.space != to.space
        || from.primitive != to.primitive
        || from.bg != to.bg
    {
        return Err("transition endpoints have different mathset metadata".into());
    }
    if from.splats.len() != to.splats.len() {
        return Err(format!(
            "transition endpoints have different primitive counts: {} vs {}",
            from.splats.len(),
            to.splats.len()
        ));
    }
    Ok(())
}

pub fn between(from: &MathSet, to: &MathSet, t: f32) -> Result<(MathSet, usize), String> {
    if !t.is_finite() || !(0.0..=1.0).contains(&t) {
        return Err("--t must be finite and between 0 and 1".into());
    }
    matching_endpoints(from, to)?;

    let mut out = from.clone();
    let mut moving = 0usize;
    for (i, ((a, b), result)) in from
        .splats
        .iter()
        .zip(&to.splats)
        .zip(&mut out.splats)
        .enumerate()
    {
        if a.len() != b.len() {
            return Err(format!(
                "splat {i} has different parameter counts: {} vs {}",
                a.len(),
                b.len()
            ));
        }
        if a[2..] != b[2..] {
            return Err(format!(
                "splat {i} changes a non-position parameter; positional transitions only change x/y"
            ));
        }
        if a[0] != b[0] || a[1] != b[1] {
            moving += 1;
        }
        result[0] = a[0] + (b[0] - a[0]) * t;
        result[1] = a[1] + (b[1] - a[1]) * t;
    }
    Ok((out, moving))
}

/// Interpolate two consecutive states of the same ordered primitive sequence.
///
/// Position, opacity and linear-light colour are linear in time. Positive
/// shape parameters use geometric interpolation, and angles take the shortest
/// path. This is intentionally separate from `between`: state interpolation
/// is valid only when the caller knows row identity was preserved while the
/// endpoints were fitted.
pub fn between_states(from: &MathSet, to: &MathSet, t: f32) -> Result<(MathSet, usize), String> {
    if !t.is_finite() || !(0.0..=1.0).contains(&t) {
        return Err("--t must be finite and between 0 and 1".into());
    }
    matching_endpoints(from, to)?;
    if t == 0.0 {
        return Ok((from.clone(), 0));
    }
    if t == 1.0 {
        let changing = from
            .splats
            .iter()
            .zip(&to.splats)
            .filter(|(a, b)| a != b)
            .count();
        return Ok((to.clone(), changing));
    }

    let geometric = |a: f32, b: f32| (a.ln() + (b.ln() - a.ln()) * t).exp();
    let mut out = from.clone();
    let mut changing = 0usize;
    for (i, ((a, b), result)) in from
        .splats
        .iter()
        .zip(&to.splats)
        .zip(&mut out.splats)
        .enumerate()
    {
        if a.len() != b.len() {
            return Err(format!(
                "splat {i} has different parameter counts: {} vs {}",
                a.len(),
                b.len()
            ));
        }
        if a != b {
            changing += 1;
        }
        result[0] = a[0] + (b[0] - a[0]) * t;
        result[1] = a[1] + (b[1] - a[1]) * t;
        result[2] = geometric(a[2], b[2]);
        result[3] = geometric(a[3], b[3]);
        let angle_delta =
            (b[4] - a[4] + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU)
                - std::f32::consts::PI;
        result[4] = a[4] + angle_delta * t;
        for channel in 5..8 {
            let ca = srgb_to_linear(a[channel]);
            let cb = srgb_to_linear(b[channel]);
            result[channel] = linear_to_srgb(ca + (cb - ca) * t);
        }
        result[8] = a[8] + (b[8] - a[8]) * t;
        if a.len() == 10 {
            result[9] = geometric(a[9], b[9]);
        }
    }
    Ok((out, changing))
}

/// Evaluate a saved rigid motion at time `t`. Unlike endpoint-wise linear
/// interpolation, this follows circular arcs around the recorded pivot and
/// rotates each anisotropic primitive with the group.
pub fn between_rigid(
    from: &MathSet,
    to: &MathSet,
    motion: &MotionSet,
    t: f32,
) -> Result<(MathSet, usize), String> {
    if !t.is_finite() || !(0.0..=1.0).contains(&t) {
        return Err("--t must be finite and between 0 and 1".into());
    }
    matching_endpoints(from, to)?;
    if motion.canvas != from.canvas {
        return Err("motion descriptor and mathset have different canvases".into());
    }

    let mut expected = from.clone();
    let mut out = from.clone();
    let mut assigned = vec![false; from.splats.len()];
    let mut moving = 0usize;
    for group in &motion.groups {
        let MotionGroup::Rigid2d {
            members,
            center,
            translation,
            rotation_rad,
        } = group;
        if !center.iter().chain(translation).all(|v| v.is_finite())
            || !rotation_rad.is_finite()
        {
            return Err("motion descriptor contains a non-finite transform".into());
        }
        for &member in members {
            if member >= from.splats.len() {
                return Err(format!("motion descriptor member {member} is out of range"));
            }
            if std::mem::replace(&mut assigned[member], true) {
                return Err(format!(
                    "motion descriptor assigns primitive {member} more than once"
                ));
            }
        }
        expected = apply_rigid(&expected, members, *center, *translation, *rotation_rad);
        out = apply_rigid(
            &out,
            members,
            *center,
            [translation[0] * t, translation[1] * t],
            rotation_rad * t,
        );
        moving += members.len();
    }

    const EPS: f32 = 2e-5;
    for (i, ((a, b), e)) in from
        .splats
        .iter()
        .zip(&to.splats)
        .zip(&expected.splats)
        .enumerate()
    {
        if a.len() != b.len() {
            return Err(format!(
                "splat {i} has different parameter counts: {} vs {}",
                a.len(),
                b.len()
            ));
        }
        if a.len() != e.len()
            || b.iter()
                .zip(e)
                .any(|(actual, expected)| (actual - expected).abs() > EPS)
        {
            return Err(format!(
                "splat {i} does not match the endpoint encoded by the motion descriptor"
            ));
        }
    }
    if t == 0.0 {
        out = from.clone();
    } else if t == 1.0 {
        out = to.clone();
    }
    Ok((out, moving))
}

#[cfg(test)]
mod tests {
    use super::{between, between_rigid, between_states};
    use crate::format::MathSet;
    use crate::motion::{MotionGroup, MotionSet};

    fn endpoint(x: f32, colour: f32) -> MathSet {
        MathSet {
            mathset: 0,
            canvas: [100, 80],
            space: "srgb".into(),
            bg: [0.0; 3],
            primitive: "gauss2d".into(),
            splats: vec![vec![x, 0.2, 0.03, 0.04, 0.0, colour, 0.2, 0.3, 0.8, 1.0]],
        }
    }

    #[test]
    fn evaluates_position_at_t() {
        let (mid, moving) = between(&endpoint(0.1, 0.1), &endpoint(0.5, 0.1), 0.25).unwrap();
        assert_eq!(moving, 1);
        assert!((mid.splats[0][0] - 0.2).abs() < f32::EPSILON);
        assert_eq!(mid.splats[0][1], 0.2);
    }

    #[test]
    fn rejects_cross_fades_and_out_of_range_time() {
        assert!(between(&endpoint(0.1, 0.1), &endpoint(0.5, 0.2), 0.5).is_err());
        assert!(between(&endpoint(0.1, 0.1), &endpoint(0.5, 0.1), 1.1).is_err());
    }

    #[test]
    fn rigid_transition_follows_an_arc_and_rotates_the_primitive() {
        let mut a = endpoint(0.6, 0.1);
        a.splats[0][1] = 0.5;
        let motion = MotionSet {
            motion: 0,
            canvas: a.canvas,
            groups: vec![MotionGroup::Rigid2d {
                members: vec![0],
                center: [0.5, 0.5],
                translation: [0.0, 0.0],
                rotation_rad: std::f32::consts::FRAC_PI_2,
            }],
        };
        let mut b = a.clone();
        b.splats[0][0] = 0.5;
        b.splats[0][1] = 0.6;
        b.splats[0][4] = std::f32::consts::FRAC_PI_2;

        let (mid, moving) = between_rigid(&a, &b, &motion, 0.5).unwrap();
        let d = 0.1 / 2.0f32.sqrt();
        assert_eq!(moving, 1);
        assert!((mid.splats[0][0] - (0.5 + d)).abs() < 1e-6);
        assert!((mid.splats[0][1] - (0.5 + d)).abs() < 1e-6);
        assert!((mid.splats[0][4] - std::f32::consts::FRAC_PI_4).abs() < 1e-6);
    }

    #[test]
    fn rigid_transition_rejects_an_endpoint_that_does_not_match_its_motion() {
        let a = endpoint(0.6, 0.1);
        let b = endpoint(0.7, 0.1);
        let motion = MotionSet {
            motion: 0,
            canvas: a.canvas,
            groups: vec![MotionGroup::Rigid2d {
                members: vec![0],
                center: [0.5, 0.2],
                translation: [0.0, 0.0],
                rotation_rad: 0.0,
            }],
        };
        assert!(between_rigid(&a, &b, &motion, 0.5).is_err());
    }

    #[test]
    fn state_transition_uses_positive_shapes_and_shortest_angle_path() {
        let mut a = endpoint(0.1, 0.1);
        let mut b = endpoint(0.5, 0.9);
        a.splats[0][2] = 0.01;
        b.splats[0][2] = 0.09;
        a.splats[0][4] = 170f32.to_radians();
        b.splats[0][4] = (-170f32).to_radians();

        let (mid, changing) = between_states(&a, &b, 0.5).unwrap();
        assert_eq!(changing, 1);
        assert!((mid.splats[0][0] - 0.3).abs() < 1e-6);
        assert!((mid.splats[0][2] - 0.03).abs() < 1e-6);
        assert!((mid.splats[0][4] - std::f32::consts::PI).abs() < 1e-6);
        assert!(mid.splats[0][5] > 0.1 && mid.splats[0][5] < 0.9);
    }
}
