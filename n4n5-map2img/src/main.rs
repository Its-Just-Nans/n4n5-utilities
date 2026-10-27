use std::num::NonZeroU32;
use std::path::PathBuf;
use std::process::exit;

use maplibre_native::CameraUpdate;
use maplibre_native::Image;
use maplibre_native::ImageRendererBuilder;
use maplibre_native::LatLng;

#[derive(Debug)]
struct AppArgs {
    style: url::Url,
    lat: f64,
    lng: f64,
    zoom: f64,
    width: NonZeroU32,
    height: NonZeroU32,
    bearing: f64,
    pitch: f64,
    output: PathBuf,
}
fn parse_path(s: &std::ffi::OsStr) -> Result<PathBuf, &'static str> {
    Ok(s.into())
}

fn parse_url(s: &std::ffi::OsStr) -> Result<url::Url, &'static str> {
    let s = s.to_str().ok_or("invalid UTF-8")?;
    s.parse().map_err(|_| "invalid URL")
}

fn parse_args() -> Result<AppArgs, pico_args::Error> {
    let mut pargs = pico_args::Arguments::from_env();

    // Help has a higher priority and should be handled separately.
    if pargs.contains(["-h", "--help"]) {
        print!(
            "Usage: BIN [OPTIONS] --lat <LAT> --lng <LNG>

Options:
    --style <STYLE>        Map style URL
    --lat <LAT>            Latitude
    --lng <LNG>            Longitude
-w, --width <WIDTH>        Image width (default: 512)
-h, --height <HEIGHT>      Image height (default: 512)
-z, --zoom <ZOOM>          Map zoom level (default: 13)
-b, --bearing <BEARING>    Map bearing in degrees (default: 0)
-p, --pitch <PITCH>        Map pitch in degrees (default: 0)
-o, --output <OUTPUT>      Output file path (default: map.png)
    --help                 Print help information"
        );
        exit(0);
    }

    let args = AppArgs {
        style: pargs
            .opt_value_from_os_str("--style", parse_url)?
            .unwrap_or("https://cartes.app/api/styles?key=base".parse().unwrap()),
        lat: pargs.value_from_str("--lat")?,
        lng: pargs.value_from_str("--lng")?,
        width: pargs
            .opt_value_from_str(["-w", "--width"])?
            .unwrap_or(NonZeroU32::new(512).unwrap()),
        height: pargs
            .opt_value_from_str(["-h", "--height"])?
            .unwrap_or(NonZeroU32::new(512).unwrap()),
        zoom: pargs.opt_value_from_str(["-z", "--zoom"])?.unwrap_or(13.0),
        bearing: pargs
            .opt_value_from_str(["-b", "--bearing"])?
            .unwrap_or(0.0),
        pitch: pargs.opt_value_from_str(["-p", "--pitch"])?.unwrap_or(0.0),
        output: pargs
            .opt_value_from_os_str(["-o", "--output"], parse_path)?
            .unwrap_or(PathBuf::from("map.png")),
    };

    // It's up to the caller what to do with the remaining arguments.
    let remaining = pargs.finish();
    if !remaining.is_empty() {
        eprintln!("Warning: unused arguments left: {:?}.", remaining);
    }

    Ok(args)
}

fn main() {
    let args = match parse_args() {
        Ok(arg) => arg,
        Err(err) => {
            eprintln!("{}", err);
            exit(1);
        }
    };

    let mut renderer = ImageRendererBuilder::new()
        .with_size(args.width, args.height)
        .build_static_renderer();
    renderer.load_style_from_url(&args.style);
    let camera = CameraUpdate::new()
        .center(LatLng {
            lat: args.lat,
            lng: args.lng,
        })
        .bearing(args.bearing)
        .pitch(args.pitch)
        .zoom(args.zoom);

    let image: Image = match renderer.render_static(&camera) {
        Ok(image) => image,
        Err(err) => {
            eprintln!("{}", err);
            exit(1);
        }
    };
    if let Err(err) = image.as_image().save(args.output) {
        eprintln!("{}", err);
        exit(1);
    }
}
