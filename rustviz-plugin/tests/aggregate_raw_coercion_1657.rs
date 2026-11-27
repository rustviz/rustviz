fn main() {
    let mut x = 0;
    let source = &mut x;
    let ptr: Option<*const i32> = Some(source);
    *source = 3;
    std::hint::black_box(ptr);
}
