fn main() {
    let mut iter = vec![Some(())].into_iter();
    for item in &mut iter {
        let _: Option<()> = item;
    }
    println!("{}", iter.len());
}
