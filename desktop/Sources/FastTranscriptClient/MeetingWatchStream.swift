import Foundation

public struct MeetingWatchEvent: Decodable, Sendable {
    public let version: Int
    public let sequence: UInt64
    public let type: String
    public let session_id: String?
    public let data: Payload

    public struct Payload: Decodable, Sendable {
        public let requires_accept: Bool?
        public let episode: UInt64?
        public let apps: [String]?
        public let active: [String]?
        public let complete: Bool?
        public let event: MeetingEvent?
        public let status: Int32?
        public let reason: String?
    }
}

/// Validates transport/lifecycle only. Rust owns attribution and admission policy.
public struct MeetingWatchStreamDecoder {
    private var lines = JSONLines()
    private var sequence: UInt64 = 0
    private var ready = false
    private var terminal = false
    private var episode: UInt64?
    private var meeting: MeetingStreamDecoder?
    public private(set) var cancelled = false

    public init() {}

    public mutating func append(_ bytes: Data) throws -> [MeetingWatchEvent] {
        var result: [MeetingWatchEvent] = []
        for line in lines.append(bytes) {
            let event = try JSONDecoder().decode(MeetingWatchEvent.self, from: line)
            guard !terminal, event.version == 1, event.sequence == sequence,
                  sequence < UInt64.max, event.session_id == nil else { throw ClientError("Unsupported or out-of-order automatic meeting event.") }
            if event.type == "watch_ready" {
                guard !ready, event.data.requires_accept == true else { throw ClientError("Unexpected automatic meeting readiness.") }
                ready = true
            } else {
                guard ready else { throw ClientError("Automatic meeting event arrived before readiness.") }
                switch event.type {
                case "accepted":
                    guard meeting == nil, let value = event.data.episode, value > 0 else { throw ClientError("Automatic meetings overlap.") }
                    episode = value
                    meeting = MeetingStreamDecoder(mode: .live)
                case "meeting":
                    guard event.data.episode == episode, meeting != nil, let inner = event.data.event else { throw ClientError("Meeting event names an inactive automatic episode.") }
                    try meeting!.consume(inner)
                case "session_exit":
                    guard event.data.episode == episode, let decoder = meeting,
                          event.data.status == 0, decoder.completed else { throw ClientError("Automatic meeting ended without a completed transcript.") }
                    try decoder.finish(exitStatus: 0)
                    episode = nil; meeting = nil
                case "offer", "offer_expired", "covered":
                    guard let value = event.data.episode, value > 0 else { throw ClientError("Automatic meeting offer has no episode.") }
                    if event.type == "offer" {
                        guard meeting == nil, let apps = event.data.apps, !apps.isEmpty else { throw ClientError("Invalid automatic meeting offer.") }
                    }
                case "call_status":
                    guard event.data.active != nil, event.data.complete != nil, event.data.episode != nil else { throw ClientError("Incomplete automatic call status.") }
                case "suppressed": break
                case "watch_cancelled": terminal = true; cancelled = true
                case "watch_completed":
                    guard meeting == nil else { throw ClientError("Watcher completed with an unfinished meeting.") }
                    terminal = true
                default: throw ClientError("Unsupported automatic meeting event: \(event.type)")
                }
            }
            sequence += 1
            result.append(event)
        }
        return result
    }

    public func finish(exitStatus: Int32) throws {
        try lines.finish()
        guard ready, terminal, exitStatus == 0 else { throw ClientError("Automatic meeting watcher ended unexpectedly (status \(exitStatus)).") }
    }
}
