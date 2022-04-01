fn main() {
    let mut x = String::from("ABC");
    let mut y = String::from("DEF");
    let mut z = &mut y;
    let guard = 1;
    if guard == 1 {
        z = &mut x;
        String::push_str(z, ",");
    }
    else {
        String::push_str(z, ",");
    }
}