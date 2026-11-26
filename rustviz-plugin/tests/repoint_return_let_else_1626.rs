fn main() {
    let mut a = String::from("a");
    let mut b = String::from("b");
    let mut r = &mut a;
    let Some(_) = std::env::args().nth(1) else {
        r = &mut b;
        return;
    };
    *r = String::new();
}

// The last arm visited returns. Either of the other two can reach the write.
fn match_write(choice: u8) {
    let mut left = String::from("left");
    let mut right = String::from("right");
    let mut leaving = String::from("leaving");
    let mut cursor = &mut left;
    match choice {
        0 => (),
        1 => { cursor = &mut right; }
        _ => {
            cursor = &mut leaving;
            return;
        }
    }
    *cursor = String::new();
}
