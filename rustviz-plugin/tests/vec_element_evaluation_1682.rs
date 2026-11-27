// The first element takes the old value before the second replaces it.
fn main() {
    let mut item = String::from("old");
    let values = vec![
        item,
        { item = String::from("replacement"); format!("other") },
    ];
    assert_eq!(values[0], "old");
    assert_eq!(item, "replacement");

    let count = 7;
    let counts = vec![count, format!("xx").len()];
    assert_eq!(count, counts[0]);
}
