// Compile an HTML scene on the desktop and save it as Director project JSON.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mut opts = director_html::CompileOptions::default();
    if a.len() > 3 { opts.width = a[3].parse().unwrap(); opts.height = a[4].parse().unwrap(); }
    let c = director_html::compile_file(std::path::Path::new(&a[1]), opts).expect("compile");
    println!("{} nodes, {}x{}", c.node_count, c.director.width, c.director.height);
    let doc = director_core::scene_export::export_scene(&c.director).expect("export");
    std::fs::write(&a[2], doc.to_json().expect("json")).expect("write");
}
