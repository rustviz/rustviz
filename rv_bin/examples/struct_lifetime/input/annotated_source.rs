struct Excerpt&lt;'a&gt; {
    <tspan data-hash="2">p</tspan>: &amp;'a str,
}

fn some_function() {
    let <tspan data-hash="4">n</tspan> = <tspan class="fn" data-hash="0" hash="5">String::from</tspan>("Ok. I'm fine.");
    let <tspan data-hash="3">first</tspan> = <tspan data-hash="4">n</tspan>.split('.').next().expect("Could not find a '.'");
    let <tspan data-hash="1">i</tspan> = Excerpt {
        <tspan data-hash="2">p</tspan>: <tspan data-hash="3">first</tspan>,
    };
    <tspan class="fn" data-hash="0" hash="6">println!</tspan>("{}", <tspan data-hash="3">first</tspan>);
    // 'i' cannot be returned be returned
    // because the struct outlives 'n'
}

fn main() {
    some_function();
}