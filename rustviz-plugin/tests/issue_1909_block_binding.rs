fn choose(v: &[String]) {
    if let Some(x) = v.last() {
        let s3 = 'blk: {
            if v.len() > 1 { break 'blk String::from("rust"); }
            String::from("hi")
        };
    }
}
