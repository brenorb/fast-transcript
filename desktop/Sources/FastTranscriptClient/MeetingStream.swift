import Foundation

/// Presentation metadata only. Capture/recovery policy remains in fscript.
public struct MeetingManifest: Decodable, Equatable, Sendable {
    public let state: String
    public let track_frames: [UInt64]
    public let route_interruptions: [RouteInterruption]?
    public let calendar_event: CalendarEvent?

    public struct CalendarEvent: Decodable, Equatable, Sendable {
        public let title: String
    }

    public struct RouteInterruption: Decodable, Equatable, Sendable {
        public let track: String
        public let reason: String
        public let resolution: String?
    }

    public var hasCoverageChanges: Bool { state == "interrupted" || !(route_interruptions ?? []).isEmpty }
}

public struct MeetingEvent: Decodable, Sendable {
    public let version: Int
    public let sequence: UInt64
    public let type: String
    public let session_id: String?
    public let data: Payload

    public struct Payload: Decodable, Sendable {
        public let requires_start: Bool?
        public let directory: String?
        public let output_path: String?
        public let text: String?
        public let segments: [TranscriptSegment]?
        public let result: TranscriptSnapshot?
        public let manifest: MeetingManifest?
        public let track: String?
        public let state: String?
        public let reason: String?
        public let message: String?
        public let fatal: Bool?
        public let recording_continues: Bool?
        public let audio_stopped: Bool?
        public let recoverable: Bool?
    }
}

/// Validates one meeting lifecycle, not recognition/capture logic. Readiness of
/// capture and model loading is independent; final text replaces preview text.
public struct MeetingStreamDecoder {
    public enum Mode: Sendable { case live, saved }
    private let mode: Mode
    private var lines = JSONLines()
    private var sequence: UInt64 = 0
    private var ready = false
    private var id: String?
    private var directory: String?
    private var modelReady = false
    private var recording = false
    private var audioSaved = false
    private var savedManifest: MeetingManifest?
    private var recognitionFailed = false
    private var terminal = false
    public private(set) var completed = false
    public private(set) var cancelled = false
    public private(set) var errorMessage: String?

    public init(mode: Mode) { self.mode = mode }

    public mutating func append(_ bytes: Data) throws -> [MeetingEvent] {
        var events: [MeetingEvent] = []
        for line in lines.append(bytes) {
            let event = try JSONDecoder().decode(MeetingEvent.self, from: line)
            try consume(event)
            events.append(event)
        }
        return events
    }

    /// Shared validation for direct and watcher-wrapped meeting events.
    public mutating func consume(_ event: MeetingEvent) throws {
        guard !terminal, event.version == 1, event.sequence == sequence, sequence < UInt64.max else {
            throw ClientError("Unsupported or out-of-order meeting event.")
        }
        if event.type == "error" {
            guard event.data.fatal == true, let message = event.data.message, !message.isEmpty,
                  event.session_id == nil || event.session_id == id else { throw ClientError("Incomplete meeting failure.") }
            terminal = true
            errorMessage = message
        } else if event.type == "ready" {
            guard !ready, id == nil, event.session_id == nil,
                  event.data.requires_start == (mode == .live) else { throw ClientError("Unexpected meeting readiness event.") }
            ready = true
        } else if event.type == "started" {
            guard ready, id == nil, let session = event.session_id, !session.isEmpty,
                  let path = event.data.directory, path.hasPrefix("/") else { throw ClientError("Meeting did not supply its session directory.") }
            id = session
            directory = path
        } else {
            guard let id, event.session_id == id else { throw ClientError("Meeting event names an inactive session.") }
            switch event.type {
            case "restored":
                guard mode == .saved, !audioSaved, let manifest = event.data.manifest,
                      ["stopped", "interrupted"].contains(manifest.state), manifest.track_frames.count == 2,
                      event.data.directory == directory else { throw ClientError("Invalid restored meeting.") }
                audioSaved = true
                savedManifest = manifest
            case "recording":
                guard mode == .live, !recording, !audioSaved, event.data.directory == directory else { throw ClientError("Unexpected meeting capture readiness.") }
                recording = true
            case "source_status":
                guard mode == .live, !audioSaved, ["microphone", "system"].contains(event.data.track ?? ""),
                      ["ready", "reconnecting", "receiving"].contains(event.data.state ?? "") else { throw ClientError("Invalid meeting source status.") }
            case "model_ready":
                guard !modelReady, !recognitionFailed else { throw ClientError("Unexpected meeting model readiness.") }
                modelReady = true
            case "update":
                guard mode == .live, modelReady, !recognitionFailed, event.data.text != nil else { throw ClientError("Invalid live meeting update.") }
            case "recognition_error":
                guard !recognitionFailed, let message = event.data.message, !message.isEmpty,
                      event.data.recording_continues == !audioSaved else { throw ClientError("Incomplete recognition failure.") }
                recognitionFailed = true
            case "saved":
                guard mode == .live, !audioSaved, let manifest = event.data.manifest,
                      manifest.state == "stopped", manifest.track_frames.count == 2,
                      event.data.directory == directory else { throw ClientError("Invalid saved meeting audio.") }
                audioSaved = true
                savedManifest = manifest
            case "completed":
                guard audioSaved, modelReady, !recognitionFailed, let result = event.data.result,
                      result.meeting == savedManifest,
                      event.data.directory == directory,
                      event.data.output_path == directory.map({ URL(fileURLWithPath: $0).appendingPathComponent("transcript.json").path }) else { throw ClientError("Meeting ended without a complete saved transcript.") }
                terminal = true
                completed = true
            case "cancelled":
                guard mode == .live, event.data.directory == directory,
                      event.data.audio_stopped == audioSaved,
                      event.data.recoverable == !audioSaved else { throw ClientError("Invalid meeting cancellation.") }
                terminal = true
                cancelled = true
            default: throw ClientError("Unsupported meeting event: \(event.type)")
            }
        }
        sequence += 1
    }

    public func finish(exitStatus: Int32) throws {
        try lines.finish()
        if let errorMessage { throw ClientError(errorMessage) }
        guard exitStatus == 0, completed || cancelled else { throw ClientError("Meeting ended before completion (status \(exitStatus)).") }
    }
}
