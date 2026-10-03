fn main() {
    // The catalogs are embedded with include_str!; rebuild when they change.
    println!("cargo:rerun-if-changed=locales");
}
