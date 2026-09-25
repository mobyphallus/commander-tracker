use super::{Draft, Form};
use bytes::Bytes;
use http_body_util::{BodyExt, Full, Limited};
use hyper::{
    body::Incoming, server::conn::http1, service::service_fn, Request, Response, StatusCode,
};
use hyper_util::rt::TokioIo;
use std::{
    convert::Infallible,
    net::{SocketAddr, TcpListener, UdpSocket},
    path::PathBuf,
    sync::Arc,
    time::Duration,
};
use tokio::sync::{watch, Semaphore};
type Reply = Response<Full<Bytes>>;

pub struct Server {
    address: SocketAddr,
    host: String,
    stop: watch::Sender<bool>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Server {
    pub fn start(path: PathBuf, address: SocketAddr) -> Result<Self, String> {
        let socket = socket2::Socket::new(
            socket2::Domain::for_address(address),
            socket2::Type::STREAM,
            Some(socket2::Protocol::TCP),
        )
        .map_err(|e| e.to_string())?;
        socket.set_reuse_address(true).map_err(|e| e.to_string())?;
        socket.bind(&address.into()).map_err(|e| e.to_string())?;
        socket.listen(128).map_err(|e| e.to_string())?;
        let listener: TcpListener = socket.into();
        let address = listener.local_addr().map_err(|e| e.to_string())?;
        listener.set_nonblocking(true).map_err(|e| e.to_string())?;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| e.to_string())?;
        let host = if address.ip().is_loopback() {
            address.ip().to_string()
        } else {
            local_ip().unwrap_or_else(|| "127.0.0.1".into())
        };
        let (stop, mut shutdown) = watch::channel(false);
        let thread=std::thread::Builder::new().name("phone-feedback".into()).spawn(move ||runtime.block_on(async move {
            let Ok(listener)=tokio::net::TcpListener::from_std(listener) else{return};
            let slots=Arc::new(Semaphore::new(16));
            loop {
                tokio::select! {
                    _=shutdown.changed()=>break,
                    accepted=listener.accept()=> {
                        let Ok((stream,_))=accepted else{continue};
                        let Ok(permit)=slots.clone().try_acquire_owned() else{continue};
                        let path=path.clone();
                        tokio::spawn(async move {
                            let _permit=permit;
                            let service=service_fn(move |req|handle(req,path.clone()));
                            let _=tokio::time::timeout(Duration::from_secs(20), http1::Builder::new().keep_alive(false).serve_connection(TokioIo::new(stream),service)).await;
                        });
                    }
                }
            }
        })).map_err(|e|e.to_string())?;
        Ok(Self {
            address,
            host,
            stop,
            thread: Some(thread),
        })
    }
    pub fn base_url(&self) -> String {
        format!("http://{}:{}", self.host, self.address.port())
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.stop.send(true);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
fn local_ip() -> Option<String> {
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("192.0.2.1:9").ok()?; // Route lookup only; no packet is sent.
    Some(socket.local_addr().ok()?.ip().to_string())
}
fn reply(status: StatusCode, body: String) -> Reply {
    Response::builder().status(status).header("Content-Type","text/html; charset=utf-8")
        .header("Cache-Control","no-store").header("Referrer-Policy","no-referrer")
        .header("X-Content-Type-Options","nosniff")
        .header("Content-Security-Policy","default-src 'none'; style-src 'unsafe-inline'; form-action 'self'; frame-ancestors 'none'; base-uri 'none'")
        .body(Full::new(Bytes::from(body))).unwrap()
}
async fn handle(req: Request<Incoming>, path: PathBuf) -> Result<Reply, Infallible> {
    let route = req.uri().path().to_string();
    let Some(token) = route
        .strip_prefix("/f/")
        .filter(|t| t.len() == 32 && t.bytes().all(|b| b.is_ascii_hexdigit()))
    else {
        return Ok(reply(
            StatusCode::NOT_FOUND,
            page(
                "Use your personal link",
                "<p>Scan the QR code on your eliminated player tile in Commander Pod.</p>",
            ),
        ));
    };
    let method = req.method().clone();
    if method != hyper::Method::GET && method != hyper::Method::POST {
        return Ok(reply(
            StatusCode::METHOD_NOT_ALLOWED,
            page(
                "Not supported",
                "<p>Open your feedback link in a browser.</p>",
            ),
        ));
    }
    let mut draft = None;
    if method == hyper::Method::POST {
        let host = req
            .headers()
            .get("host")
            .and_then(|h| h.to_str().ok())
            .unwrap_or("");
        if let Some(origin) = req.headers().get("origin") {
            if origin.to_str().ok() != Some(format!("http://{host}").as_str()) {
                return Ok(reply(
                    StatusCode::FORBIDDEN,
                    page(
                        "Please reopen your link",
                        "<p>Submit feedback from your personal Commander Pod page.</p>",
                    ),
                ));
            }
        }
        let content_type = req
            .headers()
            .get("content-type")
            .and_then(|h| h.to_str().ok())
            .unwrap_or("");
        if !content_type.starts_with("application/x-www-form-urlencoded") {
            return Ok(reply(
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                page(
                    "Invalid form",
                    "<p>Reopen your link and use the feedback form.</p>",
                ),
            ));
        }
        let body = match Limited::new(req.into_body(), 32768).collect().await {
            Ok(body) => body.to_bytes(),
            Err(_) => {
                return Ok(reply(
                    StatusCode::PAYLOAD_TOO_LARGE,
                    page(
                        "Notes are too long",
                        "<p>Keep notes to 2,000 characters.</p>",
                    ),
                ))
            }
        };
        let mut fields = std::collections::HashMap::new();
        for (key, value) in url::form_urlencoded::parse(&body) {
            if fields.insert(key.to_string(), value.to_string()).is_some() {
                return Ok(reply(
                    StatusCode::BAD_REQUEST,
                    page("Invalid form", "<p>Please reopen your link.</p>"),
                ));
            }
        }
        draft = Some(Draft {
            rating: fields.remove("rating").unwrap_or_default(),
            problem: fields.remove("problem").unwrap_or_default(),
            kingmaker: fields.remove("kingmaker").unwrap_or_default(),
            notes: fields.remove("notes").unwrap_or_default(),
        });
    }
    let token = token.to_owned();
    // SQLite work runs off the async listener so one disk write cannot hold all phones.
    let result = tokio::task::spawn_blocking(move || -> Result<Reply, ()> {
        let mut conn = rusqlite::Connection::open_with_flags(
            path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE,
        )
        .map_err(|_| ())?;
        conn.busy_timeout(Duration::from_secs(2)).map_err(|_| ())?;
        conn.pragma_update(None, "foreign_keys", true)
            .map_err(|_| ())?;
        let form = match super::form(&conn, &token) {
            Ok(form) => form,
            Err(error) => {
                return Ok(reply(
                    StatusCode::NOT_FOUND,
                    page(
                        "Feedback unavailable",
                        &format!("<p>{}</p>", escape(&error)),
                    ),
                ))
            }
        };
        if let Some(draft) = draft {
            match super::submit(&mut conn, &token, &draft) {
                Ok(()) => {
                    let mut response = reply(StatusCode::SEE_OTHER, String::new());
                    response
                        .headers_mut()
                        .insert("Location", format!("/f/{token}").parse().unwrap());
                    Ok(response)
                }
                Err(error) => Ok(reply(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    render(&token, &form, Some((&draft, &error))),
                )),
            }
        } else {
            Ok(reply(StatusCode::OK, render(&token, &form, None)))
        }
    })
    .await;
    Ok(result.ok().and_then(Result::ok).unwrap_or_else(||reply(StatusCode::SERVICE_UNAVAILABLE,page("Please try again","<p>The table couldn't load feedback right now. Your feedback has not been submitted.</p>"))))
}
fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}
fn page(title: &str, body: &str) -> String {
    format!(
        r#"<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>{}</title><style>
    :root{{color-scheme:dark;font-family:system-ui,sans-serif;background:#101114;color:#f3f4f7}}*{{box-sizing:border-box}}body{{margin:0;padding:24px 16px}}main{{max-width:540px;margin:auto}}h1{{font-size:30px;letter-spacing:-.7px;margin:12px 0}}p,small{{line-height:1.5;color:#adb1bd}}.eyebrow{{color:#b69aff;font-size:13px;letter-spacing:1.5px;text-transform:uppercase}}fieldset{{border:0;padding:0;margin:28px 0}}legend,label.title{{display:block;font-weight:600;margin:0 0 12px}}.ratings{{display:flex;gap:8px}}.rating{{flex:1;text-align:center}}.rating input{{position:absolute;opacity:0}}.rating span{{display:block;border:1px solid #363a44;border-radius:12px;padding:18px 0;background:#191b20;font-size:22px}}input:checked+span{{background:#2b2340;border-color:#b69aff}}input:focus-visible+span,select:focus-visible,textarea:focus-visible,button:focus-visible{{outline:3px solid #b69aff;outline-offset:3px}}select,textarea{{font:inherit;color:inherit;background:#191b20;border:1px solid #363a44;border-radius:12px;padding:16px;width:100%;margin-bottom:24px}}textarea{{resize:vertical;min-height:140px}}button{{border:0;border-radius:12px;background:#7040cf;color:white;padding:18px;font:600 18px system-ui;width:100%;cursor:pointer}}.notice{{padding:16px;border-radius:12px;background:#24272e;margin:20px 0}}.error{{border-left:4px solid #e5484d}}.ends{{display:flex;justify-content:space-between;margin-top:8px;font-size:13px}}footer{{margin:24px 0;font-size:13px;color:#adb1bd}}</style></head><body><main><div class="eyebrow">Commander Pod · Match feedback</div><h1>{}</h1>{}</main></body></html>"#,
        escape(title),
        escape(title),
        body
    )
}
fn render(token: &str, form: &Form, error: Option<(&Draft, &str)>) -> String {
    let draft = error.map(|p| p.0).unwrap_or(&form.draft);
    let mut body=format!("<p>Submitting as <strong>{}</strong>. Your feedback is linked to you and this match, and is visible in the table's match history.</p>",escape(&form.name));
    if let Some((_, error)) = error {
        body += &format!(
            "<div class='notice error' role='alert'>{}</div>",
            escape(error)
        );
    } else if form.submitted {
        body+="<div class='notice' role='status'>Feedback saved. You can update your answers below.</div>";
    }
    body+=&format!("<form method='post' action='/f/{token}'><fieldset><legend>How was this game?</legend><div class='ratings'>");
    for rating in 1..=5 {
        body+=&format!("<label class='rating'><input type='radio' name='rating' value='{rating}' required {}><span>{rating}</span></label>",if draft.rating==rating.to_string(){"checked"}else{""});
    }
    body+="</div><div class='ends'><span>1 · Not enjoyable</span><span>5 · Great game</span></div></fieldset>";
    for (key, label, value) in [
        ("problem", "Problem player, if any", &draft.problem),
        ("kingmaker", "Kingmaker, if any", &draft.kingmaker),
    ] {
        body += &format!("<label class='title' for='{key}'>{label}</label>");
        if key == "kingmaker" {
            body += "<p><small>Someone whose choices handed another player the win.</small></p>";
        }
        body +=
            &format!("<select id='{key}' name='{key}'><option value=''>None / not sure</option>");
        for (seat, name) in &form.players {
            body += &format!(
                "<option value='{seat}' {}>{}</option>",
                if value == &seat.to_string() {
                    "selected"
                } else {
                    ""
                },
                escape(name)
            );
        }
        body += "</select>";
    }
    body+=&format!("<label class='title' for='notes'>Notes <small>(optional)</small></label><textarea id='notes' name='notes' maxlength='2000' placeholder='What worked, what felt unfair, or anything to remember for next time…'>{}</textarea><button type='submit'>{}</button></form><footer>Keep Commander Pod open on the laptop. Use the same Wi-Fi or local network. This personal link edits only your own feedback.</footer>",escape(&draft.notes),if form.submitted{"Update feedback"}else{"Save feedback"});
    page("Rate your game", &body)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn html_escapes_player_names_and_notes_and_shows_saved_answers() {
        let form = Form {
            key: "match".into(),
            seat: 0,
            name: "<script>bad</script>".into(),
            players: vec![(0, "A & B".into())],
            draft: Draft {
                rating: "4".into(),
                problem: "0".into(),
                kingmaker: String::new(),
                notes: "</textarea><script>bad</script>".into(),
            },
            submitted: true,
        };
        let html = render(&"a".repeat(32), &form, None);
        assert!(!html.contains("<script>"));
        assert!(html.contains("&lt;/textarea&gt;"));
        assert!(html.contains("A &amp; B"));
        assert!(html.contains("value='4' required checked"));
        assert!(html.contains("value='0' selected"));
        assert!(html.contains("Feedback saved"));
        let preview = Form {
            key: "preview".into(),
            seat: 0,
            name: "Ada".into(),
            players: vec![
                (0, "Ada".into()),
                (1, "Bo".into()),
                (2, "Casey".into()),
                (3, "Drew".into()),
            ],
            draft: Draft::default(),
            submitted: false,
        };
        std::fs::write(
            std::env::temp_dir().join("commander-feedback-form.html"),
            render(&"a".repeat(32), &preview, None),
        )
        .unwrap();
    }
    #[test]
    fn http_round_trip_rejects_bad_requests_and_saves_only_the_linked_player() {
        use std::io::{Read, Write};
        let (conn, _game, token) = crate::feedback::tests::eliminated();
        let dir = std::env::temp_dir().join(format!(
            "feedback-http-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap()
        ));
        let path = crate::storage::backup(&conn, &dir).unwrap();
        let server = Server::start(path.clone(), ([127, 0, 0, 1], 0).into()).unwrap();
        let request = |method: &str, route: &str, extra: &str, body: &str| {
            let mut socket = std::net::TcpStream::connect(server.address).unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            write!(socket,"{method} {route} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\nContent-Length: {}\r\n{extra}\r\n{body}",server.address,body.len()).unwrap();
            let mut raw = String::new();
            socket.read_to_string(&mut raw).unwrap();
            raw
        };
        let route = format!("/f/{token}");
        let get = request("GET", &route, "", "");
        assert!(get.starts_with("HTTP/1.1 200"));
        assert!(get.contains("Ada"));
        assert!(get.contains("no-store"));
        let fields = "rating=5&problem=1&kingmaker=&notes=Good+game";
        let content_type = "Content-Type: application/x-www-form-urlencoded\r\n";
        let rejected = request(
            "POST",
            &route,
            &format!("{content_type}Origin: http://unrelated.example\r\n"),
            fields,
        );
        assert!(rejected.starts_with("HTTP/1.1 403"));
        assert!(
            request("POST", &route, content_type, "rating=6&notes=bad").starts_with("HTTP/1.1 422")
        );
        assert!(request(
            "POST",
            &route,
            content_type,
            &format!("rating=5&notes={}", "x".repeat(33000))
        )
        .starts_with("HTTP/1.1 413"));
        assert!(
            request("GET", "/f/00000000000000000000000000000000", "", "")
                .starts_with("HTTP/1.1 404")
        );
        let submitted = request("POST", &route, content_type, fields);
        assert!(submitted.starts_with("HTTP/1.1 303"));
        let saved = request("GET", &route, "", "");
        assert!(saved.contains("Feedback saved"));
        assert!(saved.contains("Good game"));
        let check = rusqlite::Connection::open(&path).unwrap();
        let (seat, rating, notes): (i64, i64, String) = check
            .query_row(
                "SELECT respondent_seat,rating,notes FROM game_feedback",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!((seat, rating, notes), (0, 5, "Good game".into()));
        let address = server.address;
        drop(server);
        let restarted = Server::start(path.clone(), address).unwrap();
        drop(restarted);
        drop(check);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
