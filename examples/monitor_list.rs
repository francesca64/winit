extern crate winit;

fn main() {
    let events_loop = winit::EventsLoop::new();
    for monitor in events_loop.get_available_monitors() {
        println!("{:#?}", monitor);
    }
    let primary_monitor = events_loop.get_primary_monitor();
    println!("\n\nPRIMARY: {:#?}", primary_monitor);
}
