fn main() {
    // The i18n! macro reads the catalogs at compile time; rebuild when they change.
    println!("cargo:rerun-if-changed=locales");
}
