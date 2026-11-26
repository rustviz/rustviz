fn f(c: bool) {
    let mut s = String::from("a");
    let old = s;
    let mut flag = false;
    'b: {
        if c { break 'b { flag = true; }; }
        s = String::new();
    }
    // drop(s);  // E0382 on the break path
    drop(old); let _ = flag;
}
