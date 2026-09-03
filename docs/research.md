# Research

The design uses the notify 8.2 API and the Linux inotify behavior documented
by the platform manual:

- notify overview: https://docs.rs/notify/latest/notify/
- notify Event: https://docs.rs/notify/latest/notify/struct.Event.html
- notify PollWatcher: https://docs.rs/notify/latest/notify/poll/struct.PollWatcher.html
- notify Config: https://docs.rs/notify/latest/notify/struct.Config.html
- Linux inotify: https://man7.org/linux/man-pages/man7/inotify.7.html

The notify documentation describes native and polling backends and notes that
network, pseudo, virtual, and large-tree conditions can limit native
watching. Event::need_rescan identifies events for which the stream may have
missed changes. PollWatcher provides a snapshot-based alternative with
configurable polling.

The Linux manual documents queue overflow and the limits of rename pairing.
The product decision is therefore to expose the backend and limitation
records, use bounded snapshots for reconciliation, and keep the guarantee
explicitly best effort. Unsupported remote and virtual environments remain
unverified unless the test host provides them.
