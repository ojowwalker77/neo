/* A positional transition between two ordered mathsets.
 *
 * Motion only has an identity when row i still means row i. This deliberately
 * accepts a narrower contract than arbitrary interpolation: the two endpoints
 * must have identical metadata and identical non-position parameters. Only x
 * and y may differ. That makes every intermediate set an explicit evaluation
 * of the recovered movement rather than a cross-fade of unrelated fits.
 */
use crate::format::MathSet;

pub fn between(from: &MathSet, to: &MathSet, t: f32) -> Result<(MathSet, usize), String> {
    if !t.is_finite() || !(0.0..=1.0).contains(&t) {
        return Err("--t must be finite and between 0 and 1".into());
    }
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

#[cfg(test)]
mod tests {
    use super::between;
    use crate::format::MathSet;

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
}
