import AppKit
import AVFoundation
import FastTranscriptClient
import Foundation
import Observation

@MainActor @Observable
final class DictationController {
    enum State { case off, preparing, ready, starting, recording, processing, stopping, failed }
    var state = State.off
    var error: String?
    var shortcutWarning: String?
    var shortcutLabel = "Control-Shift-Space"
    @ObservationIgnored var onRecording: ((Bool) -> Void)?
    @ObservationIgnored var onCompleted: ((RecordingEvent) -> Void)?
    @ObservationIgnored private var executable: URL?
    @ObservationIgnored private var directory: URL?
    @ObservationIgnored private var process: CLIProcess?
    @ObservationIgnored private var task: Task<Void, Never>?
    @ObservationIgnored private var held = false
    @ObservationIgnored private var latched = false
    @ObservationIgnored private var intentionalStop = false
    @ObservationIgnored private var shortcutClip = false
    @ObservationIgnored private let hud = DictationHUD()

    var isBusy: Bool { [.preparing, .starting, .recording, .processing, .stopping].contains(state) }
    var isRunning: Bool { task != nil }
    var canHold: Bool { ![.processing, .stopping].contains(state) && executable != nil }
    var title: String {
        switch state {
        case .off, .ready: return "Hold to dictate"
        case .preparing: return "Preparing dictation…"
        case .starting: return "Opening microphone…"
        case .recording: return latched ? "Listening · click to finish" : "Listening · release to finish"
        case .processing: return "Transcribing dictation…"
        case .stopping: return "Stopping dictation…"
        case .failed: return "Hold to retry dictation"
        }
    }

    func configure(executable: URL, directory: URL) {
        self.executable = executable
        self.directory = directory
    }

    func hold(_ pressed: Bool) {
        if pressed { latched = false }
        setHeld(pressed)
    }

    private func setHeld(_ pressed: Bool) {
        if !pressed {
            held = false
            if state == .recording && !shortcutClip { stopClip() }
            return
        }
        guard canHold, !held else { return }
        held = true
        error = nil
        if state == .ready { startClip() }
        else if state == .off || state == .failed { prepare() }
    }

    func accessiblePress() {
        if state == .recording { latched = false; held = false; stopClip() }
        else if state == .starting { latched = false; setHeld(false) }
        else { latched = true; setHeld(true) }
    }

    func releasePhysicalHold() { if !latched { setHeld(false) } }

    func windowClosed() {
        if !shortcutClip {
            latched = false
            setHeld(false)
        }
    }

    private func prepare() {
        guard let executable, let directory, task == nil else { return }
        state = .preparing
        intentionalStop = false
        task = Task {
            // Authorize only real capture. Explicit WAV fixtures never open a
            // microphone and must not prompt for an unrelated system grant.
            // The backend owns all capture, inference, and shortcut work.
            let permitted: Bool
            if ProcessInfo.processInfo.environment["FSCRIPT_CAPTURE_TEST_WAV"] != nil {
                permitted = true
            } else {
                switch AVCaptureDevice.authorizationStatus(for: .audio) {
                case .authorized: permitted = true
                case .notDetermined: permitted = await AVCaptureDevice.requestAccess(for: .audio)
                default: permitted = false
                }
            }
            guard !Task.isCancelled else { task = nil; state = .off; return }
            guard permitted else {
                error = "Allow Fast Transcript in System Settings > Privacy & Security > Microphone, then hold to try again."
                state = .failed; held = false; task = nil; return
            }
            let process = CLIProcess()
            self.process = process
            var decoder = RecordingStreamDecoder()
            var diagnostics = Data()
            var exitStatus: Int32?
            defer { self.process = nil; task = nil; held = false; latched = false; hud.hide() }
            do {
                for try await event in process.events(executable: executable, arguments: ["record", "--control", "--hotkey", "--output-dir", directory.path, "--quiet"], controlInput: true) {
                    switch event {
                    case .stdout(let bytes):
                        for message in try decoder.append(bytes) {
                            switch message.type {
                            case "ready":
                                shortcutWarning = message.data.shortcut_error
                                shortcutLabel = message.data.shortcut_label ?? "Shortcut unavailable"
                                state = .ready
                                if held { startClip() }
                            case "recording":
                                shortcutClip = message.data.trigger == "shortcut"
                                state = .recording
                                error = nil
                                onRecording?(shortcutClip)
                                if shortcutClip { hud.show("Listening · release \(shortcutLabel) to finish", recording: true) }
                                else if latched { hud.show("Listening · finish with Command-Shift-D", recording: true) }
                                else if !held { stopClip() }
                            case "processing":
                                state = .processing
                                if shortcutClip || latched { hud.show("Transcribing…", recording: false) }
                            case "completed":
                                state = .ready
                                held = false
                                latched = false
                                hud.hide()
                                onCompleted?(message)
                            case "cancelled": state = .ready; held = false; latched = false; hud.hide()
                            case "error":
                                error = message.data.message
                                held = false
                                latched = false
                                hud.hide()
                                if message.data.fatal == true { state = .failed }
                                else if state != .recording { state = .ready }
                            default: break
                            }
                        }
                    case .stderr(let bytes):
                        diagnostics.append(bytes)
                        if diagnostics.count > 8192 { diagnostics = Data(diagnostics.suffix(8192)) }
                    case .exited(let code): exitStatus = code
                    }
                }
                if intentionalStop { state = .off; return }
                try decoder.finish(exitStatus: exitStatus ?? 1)
                state = .off
            } catch {
                process.cancel()
                if intentionalStop { state = .off }
                else {
                    let detail = String(decoding: diagnostics, as: UTF8.self).trimmingCharacters(in: .whitespacesAndNewlines)
                    self.error = decoder.errorMessage ?? (detail.isEmpty ? error.localizedDescription : detail)
                    state = .failed
                }
            }
        }
    }

    private func send(_ action: String) {
        guard let process else { return }
        Task {
            do { try await process.writeInput(Data("{\"action\":\"\(action)\"}\n".utf8)) }
            catch { if !intentionalStop { self.error = error.localizedDescription; cancel() } }
        }
    }

    private func startClip() { state = .starting; send("start") }
    private func stopClip() { state = .processing; send("stop") }

    func cancel() {
        held = false
        latched = false
        intentionalStop = true
        hud.hide()
        if process != nil { state = .stopping; process?.cancel() }
        else { task?.cancel(); state = .off }
    }

    func shutdown() async {
        let pending = task
        let hasProcess = process != nil
        cancel()
        // An outstanding OS permission sheet cannot be awaited on Quit.
        if hasProcess { await pending?.value }
    }
}

@MainActor
private final class DictationHUD {
    private var panel: NSPanel?
    private var label: NSTextField?
    func show(_ text: String, recording: Bool) {
        if panel == nil {
            let panel = NSPanel(contentRect: NSRect(x: 0, y: 0, width: 390, height: 64), styleMask: [.nonactivatingPanel, .borderless], backing: .buffered, defer: false)
            panel.level = .floating
            panel.isOpaque = false
            panel.backgroundColor = .clear
            panel.hasShadow = true
            panel.hidesOnDeactivate = false
            panel.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary]
            let effect = NSVisualEffectView(frame: panel.contentView!.bounds)
            effect.material = .hudWindow
            effect.state = .active
            effect.wantsLayer = true
            effect.layer?.cornerRadius = 16
            let label = NSTextField(labelWithString: text)
            label.frame = effect.bounds.insetBy(dx: 12, dy: 20)
            label.alignment = .center
            label.font = .systemFont(ofSize: 14, weight: .medium)
            effect.addSubview(label)
            panel.contentView = effect
            self.panel = panel; self.label = label
        }
        label?.stringValue = text
        label?.textColor = recording ? .systemRed : .labelColor
        if let screen = NSScreen.main {
            panel?.setFrameOrigin(NSPoint(x: screen.visibleFrame.midX - 195, y: screen.visibleFrame.minY + 44))
        }
        panel?.orderFrontRegardless()
    }
    func hide() { panel?.orderOut(nil) }
}
