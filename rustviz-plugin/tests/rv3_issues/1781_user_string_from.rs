mod other {
    pub struct String;
    impl String {
        pub fn from(_s: &str) -> String { String }
    }
}
fn main() {
    let s = other::String::from("a");
    let t = s;
}
