fn main() {
    let s = String::from("hey");
    let mut v = vec![s, String::from("hey")];
    let m = v.iter_mut().next().unwrap();
    match &v[..] {
        [] => {
        }
        [x3] => {
        }
        [x31, rest @ ..] => {
        }
    }
}
