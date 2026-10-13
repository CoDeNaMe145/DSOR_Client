//! Decode a saved message and say where it stops: `why <file> <bits>`.
fn main() {
    let mut a = std::env::args().skip(1);
    let path = a.next().unwrap();
    let bits: usize = a.next().unwrap().parse().unwrap();
    let data = std::fs::read(&path).unwrap();
    let (list, err) = dsor_proto::commands::decode_message_partial(&data, bits);
    for (i, (c, actor)) in list.iter().enumerate() {
        println!("{i:3} {:5} {} actor {:?}{}", c.id(), c.name(), actor, if c.is_unknown() { "  [UNKNOWN]" } else { "" });
    }
    if let Some(e) = err {
        println!("STOP: {e}");
    }
}
