fn inspect(_s: &String) -> bool { true }
pub fn f(x: String, gate: bool) {
    if let ref a = x && gate && let ref b = x && inspect(b) {
        println!("{a}");
    } else {
        println!("{x}");
    }
}
