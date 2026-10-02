use std::env;

use trust::{Prefix, Status};

pub fn detect(command: &[String]) -> Option<Prefix> {
    trust::detect(|name| env::var_os(name), command)
}

pub fn trust(prefix: &Prefix) {
    let Ok(certificate) = authority::certificate(&control::data()) else {
        eprintln!("asura-run: no certificate authority yet, TLS will not be decrypted");
        return;
    };

    match prefix.status(&certificate) {
        Status::Installed => {}
        Status::Uninitialised => eprintln!(
            "asura-run: {} is not initialised yet; TLS is decrypted from the next launch",
            prefix.path.display()
        ),
        Status::Missing => match prefix.install(&certificate) {
            Ok(_) => eprintln!(
                "asura-run: installed the Asura certificate into {}",
                prefix.path.display()
            ),
            Err(error) => eprintln!("asura-run: could not install the certificate: {error}"),
        },
    }
}
