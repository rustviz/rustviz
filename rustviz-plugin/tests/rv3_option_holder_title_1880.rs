pub fn first(mut v: Vec<String>) {
    let maybe = v.first();
    std::hint::black_box(maybe);
    v.clear();
}
