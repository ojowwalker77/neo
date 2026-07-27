/* mathset
 *
 *   mathset render <in.mathset> <out.png> [--scale N] [--size WxH]
 *   mathset fit    <in.png|jpg> <out.mathset> [options]
 *
 * render replays a set. It never sees a source image.
 * fit    reads an image and writes the math that draws it, then decodes its
 *        own output and reports how close that came — the round trip is the
 *        only number worth quoting, since it measures the file rather than
 *        the fitter's opinion of itself.
 */
mod fit;
mod format;
mod motion;
mod refine;
mod render;
mod transition;

use format::MathSet;
use std::path::PathBuf;

const USAGE: &str = "\
usage:
  mathset render <in.mathset> <out.png> [--scale N] [--size WxH]
                 [--limit N] [--steps N]
  mathset fit <image> <out.mathset> [options]
  mathset refine <image> <in.mathset> <out.mathset> [--iters N] [--max-side N]
                 [--preview P.png]
                 [--adapt] [--count N] [--prune F] [--cycle N]
  mathset track-group <image> <in.mathset> <out.mathset> --rect X,Y,W,H
                      [--range F] [--levels N] [--max-side N]
  mathset track-change <frame-a> <frame-b> <in.mathset> <out.mathset>
                       [--threshold N] [--range F] [--levels N] [--max-side N]
  mathset transition <from.mathset> <to.mathset> <out.mathset> --t F

render options:
  --limit N        draw only the first N primitives
  --steps N        write N frames, 1 primitive to all, as out-0001.png ...

fit options:
  --max-side N     working resolution, long edge          (default 1024)
  --passes N       coarse-to-fine steps                   (default 220)
  --budget N       stop after this many primitives        (default 200000)
  --candidates N   proposals per cell per pass            (default 32)
  --pace F         fraction of cells allowed to place     (default 0.08)
  --sigma A,B      size schedule, start to end            (default 0.11,0.0045)
  --alpha A,B      opacity range for proposals            (default 0.35,1.0)
  --beta-max F     sharpest proposal allowed              (default 6)
  --perceptual     weight error by sRGB slope (usually loses)
  --preview P.png  also write the reconstruction";

fn main() {
    if let Err(e) = go() {
        eprintln!("mathset: {e}");
        std::process::exit(1);
    }
}

fn go() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("render") => cmd_render(&args[1..]),
        Some("fit") => cmd_fit(&args[1..]),
        Some("refine") => cmd_refine(&args[1..]),
        Some("track-group") => cmd_track_group(&args[1..]),
        Some("track-change") => cmd_track_change(&args[1..]),
        Some("transition") => cmd_transition(&args[1..]),
        Some("gradcheck") => cmd_gradcheck(&args[1..]),
        _ => Err(USAGE.into()),
    }
}

fn pair(v: &str, what: &str) -> Result<(f32, f32), String> {
    let (a, b) = v.split_once(',').ok_or(format!("{what} wants A,B (got {v:?})"))?;
    Ok((
        a.trim().parse().map_err(|_| format!("{what}: {a:?}"))?,
        b.trim().parse().map_err(|_| format!("{what}: {b:?}"))?,
    ))
}

fn cmd_render(args: &[String]) -> Result<(), String> {
    let mut input: Option<PathBuf> = None;
    let mut output: Option<PathBuf> = None;
    let mut scale: f32 = 1.0;
    let mut size: Option<(u32, u32)> = None;
    let mut limit: Option<usize> = None;
    let mut steps: Option<usize> = None;

    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--scale" => {
                let v = it.next().ok_or("--scale needs a number")?;
                scale = v.parse().map_err(|_| format!("--scale {v:?} is not a number"))?;
                if scale <= 0.0 {
                    return Err("--scale must be positive".into());
                }
            }
            "--size" => {
                let v = it.next().ok_or("--size needs WxH")?;
                let (a, b) = v.split_once('x').ok_or(format!("--size {v:?} is not WxH"))?;
                size = Some((
                    a.parse().map_err(|_| format!("--size width {a:?}"))?,
                    b.parse().map_err(|_| format!("--size height {b:?}"))?,
                ));
            }
            "--limit" => {
                let v = it.next().ok_or("--limit needs a number")?;
                limit = Some(v.parse().map_err(|_| format!("--limit {v:?}"))?);
            }
            "--steps" => {
                let v = it.next().ok_or("--steps needs a number")?;
                let n: usize = v.parse().map_err(|_| format!("--steps {v:?}"))?;
                if n == 0 {
                    return Err("--steps must be at least 1".into());
                }
                steps = Some(n);
            }
            _ if input.is_none() => input = Some(a.into()),
            _ if output.is_none() => output = Some(a.into()),
            _ => return Err(format!("unexpected argument {a:?}")),
        }
    }

    let input = input.ok_or("no input .mathset")?;
    let output = output.ok_or("no output .png")?;
    let ms = MathSet::load(&input)?;

    let (w, h) = size.unwrap_or((
        ((ms.canvas[0] as f32 * scale).round() as u32).max(1),
        ((ms.canvas[1] as f32 * scale).round() as u32).max(1),
    ));

    let gpu = render::Gpu::new()?;

    if let Some(n) = steps {
        // The set is an ordered sequence, so every prefix of it is a complete
        // set in its own right. Stepping through the prefixes shows an image
        // being assembled out of its own description.
        let total = ms.splats.len();
        let stem = output.with_extension("");
        let stem = stem.to_string_lossy();
        for i in 0..n {
            // geometric, because the first few primitives carry far more of
            // the image than the last few thousand
            let t = (i + 1) as f64 / n as f64;
            let k = ((total as f64).powf(t).round() as usize).clamp(1, total);
            let rgba = render::render_with(&gpu, &ms, w, h, Some(k))?;
            let path = PathBuf::from(format!("{stem}-{:04}.png", i + 1));
            save_png(&path, &rgba, w, h)?;
            println!("  {k:>7} / {total} primitives -> {}", path.display());
        }
        return Ok(());
    }

    let rgba = render::render_with(&gpu, &ms, w, h, limit)?;
    save_png(&output, &rgba, w, h)?;

    let drawn = limit.unwrap_or(ms.splats.len()).min(ms.splats.len());
    println!(
        "{} of {} splats · {}x{} reference · rendered {}x{} -> {}",
        drawn,
        ms.splats.len(),
        ms.canvas[0],
        ms.canvas[1],
        w,
        h,
        output.display()
    );
    Ok(())
}

fn cmd_fit(args: &[String]) -> Result<(), String> {
    let mut input: Option<PathBuf> = None;
    let mut output: Option<PathBuf> = None;
    let mut preview: Option<PathBuf> = None;
    let mut o = fit::Options::default();

    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut next = |what: &str| it.next().ok_or(format!("{what} needs a value")).cloned();
        match a.as_str() {
            "--max-side" => o.max_side = next("--max-side")?.parse().map_err(|_| "--max-side")?,
            "--passes" => o.passes = next("--passes")?.parse().map_err(|_| "--passes")?,
            "--budget" => o.budget = next("--budget")?.parse().map_err(|_| "--budget")?,
            "--candidates" => {
                o.candidates = next("--candidates")?.parse().map_err(|_| "--candidates")?
            }
            "--pace" => o.pace = next("--pace")?.parse().map_err(|_| "--pace")?,
            "--beta-max" => o.beta_max = next("--beta-max")?.parse().map_err(|_| "--beta-max")?,
            "--sigma" => {
                let (a, b) = pair(&next("--sigma")?, "--sigma")?;
                o.sigma_start = a;
                o.sigma_end = b;
            }
            "--alpha" => {
                let (a, b) = pair(&next("--alpha")?, "--alpha")?;
                o.alpha_lo = a;
                o.alpha_hi = b;
            }
            "--perceptual" => o.perceptual = true,
            "--preview" => preview = Some(next("--preview")?.into()),
            _ if input.is_none() => input = Some(a.into()),
            _ if output.is_none() => output = Some(a.into()),
            _ => return Err(format!("unexpected argument {a:?}")),
        }
    }

    let input = input.ok_or("no input image")?;
    let output = output.ok_or("no output .mathset")?;
    if o.candidates == 0 || o.candidates > 64 {
        return Err("--candidates must be 1..64".into());
    }
    if !(0.0..=1.0).contains(&o.pace) {
        return Err("--pace must be 0..1".into());
    }

    let img = image::open(&input).map_err(|e| format!("{}: {e}", input.display()))?;
    let gpu = render::Gpu::new()?;

    let start = std::time::Instant::now();
    let mut last = 0u32;
    let (ms, rep) = fit::fit(&gpu, &img, &o, &mut |p, total, n| {
        if p == total || p - last >= (total / 12).max(1) {
            last = p;
            eprint!("\r  pass {p}/{total} · {n} primitives          ");
        }
    })?;
    eprintln!();
    let fit_secs = start.elapsed().as_secs_f64();

    ms.save(&output)?;

    // Reload from disk and decode that, with the ordinary decoder. Measuring
    // the in-memory set would skip the text rounding and quote a number the
    // file cannot actually deliver — the file is the deliverable, so the file
    // is what gets measured.
    let saved = MathSet::load(&output)?;
    let back = render::render_with(&gpu, &saved, rep.w, rep.h, None)?;
    let db = fit::psnr(&rep.target, &back);
    let bytes = std::fs::metadata(&output).map(|m| m.len()).unwrap_or(0);

    if let Some(p) = &preview {
        save_png(p, &back, rep.w, rep.h)?;
    }

    println!(
        "{} primitives · {}x{} · {} passes · {:.1}s",
        ms.splats.len(),
        rep.w,
        rep.h,
        rep.passes_run,
        fit_secs
    );
    println!(
        "round trip: decoded the emitted file and compared to the source — {db:.2} dB"
    );
    println!(
        "{} · {:.1} numbers per primitive · {} pixels described",
        human(bytes),
        ms.splats.iter().map(|s| s.len()).sum::<usize>() as f64 / ms.splats.len().max(1) as f64,
        rep.w as u64 * rep.h as u64
    );
    println!("-> {}", output.display());
    if let Some(p) = preview {
        println!("-> {}", p.display());
    }
    Ok(())
}

fn human(b: u64) -> String {
    if b < 1024 {
        format!("{b} B")
    } else if b < 1024 * 1024 {
        format!("{:.1} KB", b as f64 / 1024.0)
    } else {
        format!("{:.1} MB", b as f64 / (1024.0 * 1024.0))
    }
}

fn save_png(path: &std::path::Path, rgba: &[u8], w: u32, h: u32) -> Result<(), String> {
    image::save_buffer(path, rgba, w, h, image::ExtendedColorType::Rgba8)
        .map_err(|e| format!("{}: {e}", path.display()))
}

fn cmd_refine(args: &[String]) -> Result<(), String> {
    let mut pos: Vec<PathBuf> = Vec::new();
    let mut preview: Option<PathBuf> = None;
    let mut o = refine::Options::default();
    let mut every = 50u32;
    let mut max_side: Option<u32> = None;
    let mut adapt = false;
    let mut count: Option<usize> = None;
    let mut prune = 0.05f32;
    let mut cycle = 60u32;

    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut next = |w: &str| it.next().ok_or(format!("{w} needs a value")).cloned();
        match a.as_str() {
            "--iters" => o.iters = next("--iters")?.parse().map_err(|_| "--iters")?,
            "--every" => every = next("--every")?.parse().map_err(|_| "--every")?,
            "--max-side" => {
                let n = next("--max-side")?.parse().map_err(|_| "--max-side")?;
                if n == 0 {
                    return Err("--max-side must be positive".into());
                }
                max_side = Some(n);
            }
            "--lr-pos" => o.lr_pos = next("--lr-pos")?.parse().map_err(|_| "--lr-pos")?,
            "--lr-scale" => o.lr_scale = next("--lr-scale")?.parse().map_err(|_| "--lr-scale")?,
            "--lr-colour" => o.lr_colour = next("--lr-colour")?.parse().map_err(|_| "--lr-colour")?,
            "--lr-alpha" => o.lr_alpha = next("--lr-alpha")?.parse().map_err(|_| "--lr-alpha")?,
            "--lr-rot" => o.lr_rot = next("--lr-rot")?.parse().map_err(|_| "--lr-rot")?,
            "--lr-beta" => o.lr_beta = next("--lr-beta")?.parse().map_err(|_| "--lr-beta")?,
            "--preview" => preview = Some(next("--preview")?.into()),
            "--adapt" => adapt = true,
            "--count" => count = Some(next("--count")?.parse().map_err(|_| "--count")?),
            "--prune" => prune = next("--prune")?.parse().map_err(|_| "--prune")?,
            "--cycle" => cycle = next("--cycle")?.parse().map_err(|_| "--cycle")?,
            _ => pos.push(a.into()),
        }
    }
    if pos.len() != 3 {
        return Err("usage: mathset refine <image> <in.mathset> <out.mathset>".into());
    }

    let img = image::open(&pos[0]).map_err(|e| format!("{}: {e}", pos[0].display()))?;
    let ms = MathSet::load(&pos[1])?;
    let (w, h) = render::working_size(ms.canvas, max_side);
    let target = img
        .resize_exact(w, h, image::imageops::FilterType::Lanczos3)
        .to_rgba8()
        .as_raw()
        .clone();

    let gpu = render::Gpu::new()?;
    let iters = o.iters;
    let mut r = refine::Refiner::new(&gpu, &ms, &target, w, h, o)?;

    // measured the same way everywhere: decode the set with the ordinary
    // decoder and compare against the source
    let score = |gpu: &render::Gpu, set: &MathSet| -> Result<f64, String> {
        Ok(fit::psnr(&target, &render::render_with(gpu, set, w, h, None)?))
    };
    let before = score(&gpu, &ms)?;
    println!("{} primitives · {w}x{h} · start {before:.2} dB", ms.splats.len());

    let start = std::time::Instant::now();
    let target_count = count.unwrap_or(ms.splats.len());
    for i in 1..=iters {
        r.step(&gpu)?;

        // Adapt on a cycle rather than every step: prune and split both
        // invalidate the optimiser state around the primitives they touch, so
        // the set needs time to settle before it is judged again. Never on
        // the last stretch, so the run ends on a refined set.
        if adapt && i % cycle == 0 && i + cycle <= iters {
            let (dropped, added) = r.adapt(prune, target_count);
            let db = score(&gpu, &r.to_mathset(&ms))?;
            println!(
                "  iter {i:>4}/{iters} · {db:.2} dB · -{dropped} +{added} -> {} primitives",
                r.len()
            );
        } else if i % every == 0 || i == iters {
            let db = score(&gpu, &r.to_mathset(&ms))?;
            println!("  iter {i:>4}/{iters} · {db:.2} dB · {} primitives", r.len());
        }
    }

    let out = r.to_mathset(&ms);
    out.save(&pos[2])?;
    let after = score(&gpu, &out)?;
    if let Some(p) = &preview {
        save_png(p, &render::render_with(&gpu, &out, w, h, None)?, w, h)?;
    }
    println!(
        "{before:.2} -> {after:.2} dB  ({:+.2}) · {} -> {} primitives · {:.1}s",
        after - before,
        ms.splats.len(),
        out.splats.len(),
        start.elapsed().as_secs_f64()
    );
    println!("-> {}", pos[2].display());
    Ok(())
}

/// Search a rigid translation for one known group. This deliberately separates
/// two questions: whether a group move can cross the large-motion basin, and
/// how groups should be discovered. `--rect` supplies membership for the first
/// question without pretending that the second one is solved.
fn cmd_track_group(args: &[String]) -> Result<(), String> {
    let mut pos: Vec<PathBuf> = Vec::new();
    let mut rect: Option<motion::Rect> = None;
    let mut opt = motion::Options::default();

    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut next = |w: &str| it.next().ok_or(format!("{w} needs a value")).cloned();
        match a.as_str() {
            "--rect" => rect = Some(motion::Rect::parse(&next("--rect")?)?),
            "--range" => {
                opt.range = next("--range")?.parse().map_err(|_| "--range")?;
                if !opt.range.is_finite() || opt.range <= 0.0 {
                    return Err("--range must be positive and finite".into());
                }
            }
            "--levels" => {
                opt.levels = next("--levels")?.parse().map_err(|_| "--levels")?;
                if opt.levels == 0 {
                    return Err("--levels must be positive".into());
                }
            }
            "--max-side" => {
                opt.max_side = next("--max-side")?.parse().map_err(|_| "--max-side")?;
                if opt.max_side == 0 {
                    return Err("--max-side must be positive".into());
                }
            }
            _ => pos.push(a.into()),
        }
    }
    if pos.len() != 3 {
        return Err(
            "usage: mathset track-group <image> <in.mathset> <out.mathset> --rect X,Y,W,H".into(),
        );
    }
    let rect = rect.ok_or("track-group needs --rect X,Y,W,H")?;
    let img = image::open(&pos[0]).map_err(|e| format!("{}: {e}", pos[0].display()))?;
    let ms = MathSet::load(&pos[1])?;
    motion::track_group(&img, &ms, rect, &pos[2], opt)
}

/// Infer spatially separate change components directly from the two source
/// frames, recover one rigid translation per component by frame correspondence,
/// then apply those transforms to the primitives inside each component.
fn cmd_track_change(args: &[String]) -> Result<(), String> {
    let mut pos: Vec<PathBuf> = Vec::new();
    let mut threshold = 2u8;
    let mut opt = motion::Options::default();

    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut next = |w: &str| it.next().ok_or(format!("{w} needs a value")).cloned();
        match a.as_str() {
            "--threshold" => {
                threshold = next("--threshold")?.parse().map_err(|_| "--threshold")?;
                if threshold == 0 {
                    return Err("--threshold must be 1..255".into());
                }
            }
            "--range" => {
                opt.range = next("--range")?.parse().map_err(|_| "--range")?;
                if !opt.range.is_finite() || opt.range <= 0.0 {
                    return Err("--range must be positive and finite".into());
                }
            }
            "--levels" => {
                opt.levels = next("--levels")?.parse().map_err(|_| "--levels")?;
                if opt.levels == 0 {
                    return Err("--levels must be positive".into());
                }
            }
            "--max-side" => {
                opt.max_side = next("--max-side")?.parse().map_err(|_| "--max-side")?;
                if opt.max_side == 0 {
                    return Err("--max-side must be positive".into());
                }
            }
            _ => pos.push(a.into()),
        }
    }
    if pos.len() != 4 {
        return Err(
            "usage: mathset track-change <frame-a> <frame-b> <in.mathset> <out.mathset>".into(),
        );
    }

    let a = image::open(&pos[0]).map_err(|e| format!("{}: {e}", pos[0].display()))?;
    let b = image::open(&pos[1]).map_err(|e| format!("{}: {e}", pos[1].display()))?;
    let ms = MathSet::load(&pos[2])?;
    motion::track_changes(&a, &b, &ms, &pos[3], threshold, opt)
}

/// Evaluate the recovered position field at one time in [0, 1]. The endpoints
/// must share row identity and differ only in x/y; this is movement, not an
/// appearance cross-fade.
fn cmd_transition(args: &[String]) -> Result<(), String> {
    let mut pos: Vec<PathBuf> = Vec::new();
    let mut t: Option<f32> = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--t" => {
                let value = it.next().ok_or("--t needs a value")?;
                t = Some(value.parse().map_err(|_| format!("--t: {value:?}"))?);
            }
            _ => pos.push(a.into()),
        }
    }
    if pos.len() != 3 {
        return Err(
            "usage: mathset transition <from.mathset> <to.mathset> <out.mathset> --t F"
                .into(),
        );
    }
    let t = t.ok_or("transition needs --t F")?;
    let from = MathSet::load(&pos[0])?;
    let to = MathSet::load(&pos[1])?;
    let (out, moving) = transition::between(&from, &to, t)?;
    out.save(&pos[2])?;
    println!(
        "{} primitives · {moving} moving · t={t:.3}\n-> {}",
        out.splats.len(),
        pos[2].display()
    );
    Ok(())
}

/// Every analytic gradient, against a central finite difference of the same
/// forward model computed independently on the CPU. A sign error or a missing
/// chain-rule term shows up here and nowhere else — a wrong gradient still
/// improves the image, just slowly and in the wrong direction for one
/// parameter, which no fidelity number would reveal.
fn cmd_gradcheck(args: &[String]) -> Result<(), String> {
    let img = image::open(&args[0]).map_err(|e| format!("{}: {e}", args[0]))?;
    let ms = MathSet::load(std::path::Path::new(&args[1]))?;
    let (w, h) = (ms.canvas[0], ms.canvas[1]);
    let target = img
        .resize_exact(w, h, image::imageops::FilterType::Lanczos3)
        .to_rgba8()
        .as_raw()
        .clone();

    let gpu = render::Gpu::new()?;
    let mut r = refine::Refiner::new(&gpu, &ms, &target, w, h, refine::Options::default())?;
    let g = r.gradients(&gpu)?;

    let names = ["x", "y", "sx", "sy", "theta", "r", "g", "b", "alpha", "beta"];
    let steps = [2e-4f32, 2e-4, 2e-4, 2e-4, 2e-3, 2e-3, 2e-3, 2e-3, 2e-3, 2e-3];
    println!("{} primitives · {w}x{h}", ms.splats.len());
    println!("{:>6} {:>16} {:>16} {:>10}", "param", "analytic", "finite diff", "rel err");

    let mut worst = 0f64;
    for j in 0..10 {
        // sum one parameter across every primitive: a single perturbation
        // then probes the whole column at once, and any per-primitive sign
        // error fails to cancel
        let analytic: f64 = (0..ms.splats.len()).map(|k| g[k * 11 + j] as f64).sum();
        let eps = steps[j];
        for k in 0..ms.splats.len() {
            r.params_mut()[k * 10 + j] += eps;
        }
        let up = r.loss();
        for k in 0..ms.splats.len() {
            r.params_mut()[k * 10 + j] -= 2.0 * eps;
        }
        let dn = r.loss();
        for k in 0..ms.splats.len() {
            r.params_mut()[k * 10 + j] += eps;
        }
        let fd = (up - dn) / (2.0 * eps as f64);
        let rel = (analytic - fd).abs() / analytic.abs().max(fd.abs()).max(1e-9);
        worst = worst.max(rel);
        println!("{:>6} {analytic:>16.4} {fd:>16.4} {rel:>10.5}", names[j]);
    }
    // The worth score decides what gets pruned, so it gets the same
    // treatment: predicted loss increase from removing a primitive, against
    // the loss actually measured after removing it.
    println!();
    println!("{:>6} {:>16} {:>16} {:>10}", "prim", "predicted", "actual", "rel err");
    let worth = r.worth();
    let base = r.loss();
    let mut order: Vec<usize> = (0..worth.len()).collect();
    order.sort_by(|&a, &b| worth[b].partial_cmp(&worth[a]).unwrap_or(std::cmp::Ordering::Equal));
    let mut worst_w = 0f64;
    for &k in order.iter().step_by(order.len().max(6) / 6).take(6) {
        let mut probe = refine::Refiner::new(&gpu, &ms, &target, w, h, refine::Options::default())?;
        probe.drop_one(k);
        let actual = probe.loss() - base;
        let pred = worth[k] as f64;
        let rel = (pred - actual).abs() / pred.abs().max(actual.abs()).max(1e-9);
        worst_w = worst_w.max(rel);
        println!("{k:>6} {pred:>16.5} {actual:>16.5} {rel:>10.5}");
    }
    println!("worst worth error {worst_w:.5}");

    println!();
    if worst < 0.05 {
        println!("worst relative error {worst:.5} — gradients agree");
        Ok(())
    } else {
        Err(format!("worst relative error {worst:.5} — gradients disagree"))
    }
}

#[cfg(test)]
mod tests {
    use super::{motion, render};

    #[test]
    fn working_size_preserves_aspect_and_never_upscales() {
        assert_eq!(render::working_size([451, 511], Some(128)), (113, 128));
        assert_eq!(render::working_size([451, 511], Some(1024)), (451, 511));
        assert_eq!(render::working_size([1, 511], Some(64)), (1, 64));
    }

    #[test]
    fn rectangle_parsing_and_membership_are_explicit() {
        let r = motion::Rect::parse("0.35,0.25,0.30,0.30").unwrap();
        assert!(r.contains(0.35, 0.25));
        assert!(r.contains(0.65, 0.55));
        assert!(!r.contains(0.34, 0.25));
        assert!(motion::Rect::parse("0,0,0,1").is_err());
        assert!(motion::Rect::parse("0,0,NaN,1").is_err());
        assert!(motion::Rect::parse("0,0,1").is_err());
    }
}
