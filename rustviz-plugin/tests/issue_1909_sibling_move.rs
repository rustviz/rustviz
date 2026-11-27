fn main() {
    let tp = (String::from("rust"), 1);
    match tp {
        (s, 1) => drop(s),
        (s, _) => drop(s),
    }
    let Some(s) = Some(String::from("hi")) else {
        return;
    };
}

fn reads_copy_field() {
    let tp = (String::from("rust"), 1);
    match tp {
        (s, 1) => drop(s),
        (s, _) => drop(s),
    }
    let Some(s) = Some(String::from("hi")) else {
        return;
    };
    std::hint::black_box(&tp.1);
}
