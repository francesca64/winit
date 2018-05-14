extern crate winit;

fn main() {
    let mut events_loop = winit::EventsLoop::new();

    let _window = winit::WindowBuilder::new()
        .with_title("Your faithful window")
        .build(&events_loop)
        .unwrap();

    events_loop.run_forever(|event| {
        use winit::WindowEvent::*;
        match event {
            winit::Event::WindowEvent { event, .. } => match event {
                CloseRequested => return winit::ControlFlow::Break,
                KeyboardInput { input, .. } => println!("{:#?}", input),
                ReceivedCharacter(c) => println!("ReceivedCharacter {} ({:2X?})", c, c as u8),
                _ => (),
            },
            _ => (),
        }
        winit::ControlFlow::Continue
    });
}
