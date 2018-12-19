extern crate winit;

use std::collections::HashMap;
use winit::window::Window;
use winit::event::{Event, WindowEvent, ElementState, KeyboardInput, VirtualKeyCode};
use winit::event_loop::{EventLoop, ControlFlow};

fn main() {
    let event_loop = EventLoop::new();

    let mut windows = HashMap::new();
    for _ in 0..3 {
        let window = Window::new(&event_loop).unwrap();
        windows.insert(window.id(), window);
    }

    event_loop.run(move |event, event_loop, control_flow| {
        *control_flow = match !windows.is_empty() {
            true => ControlFlow::Wait,
            false => ControlFlow::Exit,
        };
        match event {
            Event::WindowEvent { event, window_id } => {
                match event {
                    WindowEvent::CloseRequested => {
                        println!("Window {:?} has received the signal to close", window_id);
                        // This drops the window, causing it to close.
                        windows.remove(&window_id);
                    },
                    WindowEvent::KeyboardInput { input: KeyboardInput { state: ElementState::Pressed, virtual_keycode, .. }, .. } => {
                        if Some(VirtualKeyCode::Escape) == virtual_keycode {
                            windows.remove(&window_id);
                        } else {
                            let window = Window::new(&event_loop).unwrap();
                            windows.insert(window.id(), window);
                        }
                    },
                    _ => (),
                }
            }
            _ => (),
        }
    })
}
