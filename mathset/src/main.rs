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
mod render;

use format::MathSet;
use std::path::PathBuf;

const USAGE: &str = "\
usage:
  mathset render <in.mathset> <out.png> [--scale N] [--size WxH]
                 [--limit N] [--steps N]
  mathset fit <image> <out.mathset> [options]

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
