// Each first operand is complete before evaluating tail?.
struct Wrapper(String);

fn tuple(left: String, right: String, tail: Option<()>) -> Option<()> {
    let value = (((left,), right), tail?);
    drop(value);
    Some(())
}

fn structure(field: String, tail: Option<()>) -> Option<()> {
    let value = (Wrapper { 0: field }, tail?);
    drop(value);
    Some(())
}

fn array(element: String, tail: Option<()>) -> Option<()> {
    let value = ([element], tail?);
    drop(value);
    Some(())
}

fn option(payload: String, tail: Option<()>) -> Option<()> {
    let value = (Some(payload), tail?);
    drop(value);
    Some(())
}

fn boxed(contents: String, tail: Option<()>) -> Option<()> {
    let value = (Box::new(contents), tail?);
    drop(value);
    Some(())
}

fn repeat_array(repeated: String, tail: Option<()>) -> Option<()> {
    let value = ([repeated; 1], tail?);
    drop(value);
    Some(())
}

fn extracted(s: String, tail: Option<()>) -> Option<()> {
    let value = (drop(Some(s).take()), tail?);
    Some(value.1)
}

fn replaced(mut s: String, tail: Option<()>) -> Option<()> {
    let value = (Some(s), {
        s = String::from("replacement");
        tail?
    });
    drop((value, s));
    Some(())
}

fn maybe_replaced(mut s: String, replace: bool, tail: Option<()>) -> Option<()> {
    let value = ((s,), {
        if replace { s = String::from("replacement"); }
        tail?
    });
    drop(value);
    Some(())
}

fn residual_operand(mut s: Result<(), String>, tail: Result<(), String>) -> Result<(), String> {
    let value = ((s,), {
        s = tail;
        s?
    });
    drop(value);
    Ok(())
}

fn ended_source_scope(tail: Option<()>) -> Option<()> {
    let value = ({
        let s = String::from("original");
        Some(s)
    }, tail?);
    drop(value);
    Some(())
}
