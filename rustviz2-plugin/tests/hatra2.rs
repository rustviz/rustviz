fn compare_strings(s1: &String, s2: &String) -> bool{
    *s1 == *s2
}

fn clear_string(s3: & mut String) {
    s3.clear();
}

fn main(){
    let mut s = String::from("hello");

    let r1 = &s;
    let r2 = &s;
    compare_strings(r1, r2);

    let r3 = &mut s;
    clear_string(r3);
}