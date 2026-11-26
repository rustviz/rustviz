fn main() {
    String::from_utf8_lossy(b"hi");
    let bytes = vec![104, 105];
    let s = String::from_utf8(bytes).unwrap();
    let w = String::from("z");
    println!("{s}{w}");
}
