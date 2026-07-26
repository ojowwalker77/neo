/* mathset — step 1: the decoder.
 *
 *   mathset render <in.mathset> <out.png> [--scale N] [--size WxH]
 *
 * No target image, no fitting, no error metric. Give it a set of math and it
 * paints. The --scale flag exists from day one because it is the argument:
 * the same numbers rendered at 8x are not upscaled, they are re-evaluated.
 */
mod format;
mod render;

use format::MathSet;
use std::path::PathBuf;

fn main() {
    if let Err(e) = go() {
        eprintln!("mathset: {e}");
        std::process::exit(1);
    }
}

fn go() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let cmd = args.next().unwrap_or_default();
    if cmd != "render" {
        return Err("usage: mathset render <in.mathset> <out.png> [--scale N] [--size WxH]".into());
    }

    let mut input: Option<PathBuf> = None;
    let mut output: Option<PathBuf> = None;
    let mut scale: f32 = 1.0;
    let mut size: Option<(u32, u32)> = None;

    while let Some(a) = args.next() {
        match a.as_str() {
            "--scale" => {
                let v = args.next().ok_or("--scale needs a number")?;
                scale = v.parse().map_err(|_| format!("--scale {v:?} is not a number"))?;
                if scale <= 0.0 {
                    return Err("--scale must be positive".into());
                }
            }
            "--size" => {
                let v = args.next().ok_or("--size needs WxH")?;
                let (w, h) = v.split_once('x').ok_or(format!("--size {v:?} is not WxH"))?;
                size = Some((
                    w.parse().map_err(|_| format!("--size width {w:?}"))?,
                    h.parse().map_err(|_| format!("--size height {h:?}"))?,
                ));
            }
            _ if input.is_none() => input = Some(a.into()),
            _ if output.is_none() => output = Some(a.into()),
            _ => return Err(format!("unexpected argument {a:?}")),
        }
    }

    let input = input.ok_or("no input .mathset")?;
    let output = output.ok_or("no output .png")?;

    let ms = MathSet::load(&input)?;

    let (w, h) = match size {
        Some(wh) => wh,
        None => (
            ((ms.canvas[0] as f32 * scale).round() as u32).max(1),
            ((ms.canvas[1] as f32 * scale).round() as u32).max(1),
        ),
    };

    let out = render::render(&ms, w, h)?;

    image::save_buffer(
        &output,
        &out.rgba,
        out.w,
        out.h,
        image::ExtendedColorType::Rgba8,
    )
    .map_err(|e| format!("{}: {e}", output.display()))?;

    println!(
        "{} splats · {}x{} reference · rendered {}x{} -> {}",
        ms.splats.len(),
        ms.canvas[0],
        ms.canvas[1],
        out.w,
        out.h,
        output.display()
    );
    Ok(())
}
