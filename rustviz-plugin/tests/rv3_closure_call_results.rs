struct Record {
    value: String,
}

fn tuple(s: String) {
    let c = move || {
        drop(s);
        String::from("tuple")
    };
    let z = (c(),);
    drop(z);
}

fn record(s: String) {
    let c = move || {
        drop(s);
        String::from("field")
    };
    let z = Record { value: c() };
    drop(z);
}

fn array(s: String) {
    let c = move || {
        drop(s);
        String::from("array")
    };
    let z = [c()];
    drop(z);
}

fn ok(s: String) {
    let c = move || {
        drop(s);
        String::from("ok")
    };
    let z: Result<_, ()> = Ok(c());
    drop(z);
}

fn break_value(s: String) {
    let c = move || {
        drop(s);
        String::from("break")
    };
    let z = 'b: { break 'b c(); };
    drop(z);
}

fn block_tail(s: String) {
    let c = move || {
        drop(s);
        String::from("block")
    };
    let z = {
        let n = 1;
        c()
    };
    drop(z);
}

fn explicit_return(s: String) -> (String,) {
    let c = move || {
        drop(s);
        String::from("return")
    };
    return (c(),);
}

fn before_try(s: String, ok: bool) -> Result<(), ()> {
    let c = move || {
        drop(s);
        if ok { Ok(String::from("success")) } else { Err(()) }
    };
    let z = c()?;
    drop(z);
    Ok(())
}

fn mutable_call(mut s: String) {
    let mut c = move || {
        s.push('!');
        s.clone()
    };
    let z = Ok::<_, ()>(c());
    let next = c();
    drop((z, next));
}

fn direct_try(s: String, ok: bool) -> Result<String, ()> {
    let c = move || {
        drop(s);
        if ok { Ok(Ok(String::from("output"))) } else { Err(()) }
    };
    return c()?;
}

fn wrapped_try(s: String, ok: bool) -> Result<String, ()> {
    let c = move || {
        drop(s);
        if ok { Ok(String::from("output")) } else { Err(()) }
    };
    return Ok(c()?);
}

fn wrapped_break(s: String) {
    let c = move || s;
    let z = 'b: { break 'b Some(c()); };
    drop(z);
}

fn copy_result(s: String) {
    let c = move || {
        drop(s);
        7
    };
    let count = c();
    std::hint::black_box(count);
}

fn shared_call(s: String) {
    let c = || s.clone();
    let first = c();
    let second = c();
    drop((first, second));
}
