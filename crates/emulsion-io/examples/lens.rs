//! Read a photo's EXIF, fetch the lensfun database if needed, and look the lens up:
//! `cargo run -p emulsion-io --example lens -- photo.jpg`
fn main() {
    let path = std::path::PathBuf::from(std::env::args().nth(1).expect("photo"));
    let info = emulsion_io::exif::read(&path);
    println!("exif: {:?}", info);
    if !emulsion_io::lensfun::installed() {
        let cancel = std::sync::atomic::AtomicBool::new(false);
        let t = std::time::Instant::now();
        emulsion_io::lensfun::install(
            &|d, n| {
                if d % 10 == 0 {
                    println!("  {d}/{n}");
                }
            },
            &cancel,
        )
        .expect("install");
        println!("installed in {:?}", t.elapsed());
    }
    let t = std::time::Instant::now();
    let db = emulsion_io::lensfun::Database::load().expect("db");
    println!(
        "db: {} cameras, {} lenses, loaded in {:?}",
        db.cameras.len(),
        db.lenses.len(),
        t.elapsed()
    );
    if let Some(i) = info {
        let cam = db.find_camera(&i.make, &i.model).map(|c| {
            format!(
                "{} {} ({}, crop {})",
                c.maker, c.model, c.mount, c.cropfactor
            )
        });
        println!("camera match: {cam:?}");
        match emulsion_io::lensfun::profile_for(
            &db, &i.make, &i.model, &i.lens, i.focal_mm, i.f_number,
        ) {
            Some(p) => println!("profile: {p:#?}"),
            None => println!("no lens profile for {:?}", i.lens),
        }
    }
}
