use std::sync::Mutex;

fn absent_payload() -> (Option<String>,) {
    let empty = || None::<String>;
    (empty(),)
}

fn unknown_payload<T>(value: T) -> (T,) {
    let generic = move || value;
    (generic(),)
}

fn synchronization_resource() -> Option<Mutex<u32>> {
    let c = || Mutex::new(7);
    Some(c())
}

fn make() -> String {
    String::from("owned")
}

fn named_function() -> (String,) {
    (make(),)
}

fn main() {
    assert_eq!(absent_payload(), (None,));
    assert_eq!(unknown_payload(7u32), (7,));
    assert_eq!(synchronization_resource().unwrap().into_inner().unwrap(), 7);
    assert_eq!(named_function(), (String::from("owned"),));
}
