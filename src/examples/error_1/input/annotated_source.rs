fn main() {
    let <tspan data-hash="1">x</tspan> = 5;
    <tspan data-hash="1">x</tspan> = 6; // ERROR: cannot assign twice to immutable variable x
}