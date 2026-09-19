//! Print a model's inputs and outputs: `cargo run -p emulsion-ai --example inspect -- path.onnx`
fn main() {
    let path = std::env::args().nth(1).expect("model path");
    let session = ort::session::Session::builder()
        .unwrap()
        .commit_from_file(&path)
        .unwrap();
    println!("{path}");
    for i in session.inputs() {
        println!("  in  {} {:?}", i.name(), i.dtype());
    }
    for o in session.outputs() {
        println!("  out {} {:?}", o.name(), o.dtype());
    }
}
