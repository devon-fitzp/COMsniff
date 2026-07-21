// COMsniff - A simple tool to sniff and analyze COM traffic
// Open COM1 and COM2 and forward data between them while logging the traffic

mod app;
mod config;
mod serial;
mod ui;

use app::App;

fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;

    let (available_ports, port_enum_error) = match serialport::available_ports() {
        Ok(ports) => (ports.into_iter().map(|p| p.port_name).collect(), None),
        Err(e) => (Vec::new(), Some(e.to_string())),
    };

    let mut app = App::new(available_ports, port_enum_error);
    ratatui::run(|terminal| app.run(terminal))?;
    Ok(())
}