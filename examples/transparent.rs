extern crate winit;

fn main() {
    let mut event_loop = winit::EventLoop::new();

    let window = winit::WindowBuilder::new().with_decorations(false)
                                                 .with_transparency(true)
                                                 .build(&event_loop).unwrap();

    window.set_title("A fantastic window!");

    event_loop.run(move |event| {
        println!("{:?}", event);

        match event {
            winit::Event::WindowEvent { event: winit::WindowEvent::CloseRequested, .. } => winit::ControlFlow::Exit,
            _ => winit::ControlFlow::Wait,
        }
    });
}
