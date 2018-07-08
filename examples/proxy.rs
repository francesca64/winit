extern crate winit;

fn main() {
    let mut event_loop = winit::EventLoop::new();

    let _window = winit::WindowBuilder::new()
        .with_title("A fantastic window!")
        .build(&event_loop)
        .unwrap();

    let proxy = event_loop.create_proxy();

    std::thread::spawn(move || {
        // Wake up the `event_loop` once every second.
        loop {
            std::thread::sleep(std::time::Duration::from_secs(1));
            proxy.wakeup().unwrap();
        }
    });

    event_loop.run(move |event| {
        println!("{:?}", event);
        match event {
            winit::Event::WindowEvent { event: winit::WindowEvent::CloseRequested, .. } =>
                winit::ControlFlow::Exit,
            _ => winit::ControlFlow::Wait,
        }
    });
}
