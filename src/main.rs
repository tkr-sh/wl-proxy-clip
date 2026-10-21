use std::path::Path;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [name, downstream, control] = args.as_slice() else {
        eprintln!("usage: wl-proxy-clip <name> <downstream-socket> <control-socket>");
        std::process::exit(2);
    };

    if let Err(error) = wl_proxy_clip::run(name, Path::new(downstream), Path::new(control)) {
        eprintln!("wl-proxy-clip: {error}");
        std::process::exit(1);
    }
}
