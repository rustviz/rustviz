macro_rules! twice {
    ($tail:expr) => {{
        'exit: {
            if true { break 'exit; }
            $tail?;
        }
        $tail?;
    }}
}

fn tail() -> Option<()> { None }

fn expanded_residuals(owner: String) -> Option<()> {
    twice!(tail());
    drop(owner);
    Some(())
}
