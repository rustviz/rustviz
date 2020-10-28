fn main(){
    let mut s = String::from("hello");

    let r1 = &s;
    let r2 = &s;
    assert!(compare_strings(r1, r2));

    let r3 = &mut s;
    clear_string(r3)
}

fn compare_strings(s1: &String , s2: &String) -> bool {
    if *s1 == *s2 {
        true
    } else {
        false
    }
}

fn clear_string(s: &mut String) {
    *s = String::from("")
}