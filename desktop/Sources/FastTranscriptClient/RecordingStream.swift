import Foundation

public struct RecordingEvent: Decodable, Sendable {
    public let version: Int
    public let sequence: UInt64
    public let type: String
    public let session_id: String?
    public let data: Payload

    public struct Payload: Decodable, Sendable {
        public let result: TranscriptSnapshot?
        public let output_path: String?
        public let model_reused: Bool?
        public let message: String?
        public let fatal: Bool?
    }
}

/// Validates presentation state only; capture/inference remain inside fscript.
public struct RecordingStreamDecoder {
    private var lines = JSONLines()
    private var sequence: UInt64 = 0
    private var seen = Set<String>()
    private enum State: Equatable { case loading, idle, recording(String), processing(String), failed }
    private var state = State.loading
    public private(set) var errorMessage: String?

    public init() {}

    public mutating func append(_ bytes: Data) throws -> [RecordingEvent] {
        var result: [RecordingEvent] = []
        for line in lines.append(bytes) {
            let event = try JSONDecoder().decode(RecordingEvent.self, from: line)
            guard state != .failed, event.version == 1, event.sequence == sequence, sequence < UInt64.max else {
                throw ClientError("Unsupported or out-of-order recording event.")
            }
            switch event.type {
            case "ready":
                guard state == .loading, event.session_id == nil else { throw ClientError("Unexpected recording readiness event.") }
                state = .idle
            case "recording":
                guard state == .idle, let id = event.session_id, !id.isEmpty, seen.insert(id).inserted else { throw ClientError("Invalid or reused recording session.") }
                state = .recording(id)
            case "processing":
                guard let id = event.session_id, state == .recording(id) else { throw ClientError("Processing requires the active recording.") }
                state = .processing(id)
            case "completed":
                guard let id = event.session_id, state == .processing(id), event.data.result != nil, event.data.model_reused != nil else { throw ClientError("Incomplete or unexpected dictation result.") }
                state = .idle
            case "cancelled":
                guard let id = event.session_id, state == .recording(id) else { throw ClientError("Cancellation requires an active recording.") }
                state = .idle
            case "error":
                guard let fatal = event.data.fatal, let message = event.data.message, !message.isEmpty else { throw ClientError("Recording error is missing its details.") }
                if fatal { state = .failed; errorMessage = message }
                else {
                    switch state {
                    case .idle: guard event.session_id == nil else { throw ClientError("Recording error names an inactive session.") }
                    case .recording(let id): guard event.session_id == id else { throw ClientError("Recording error names a different session.") }
                    case .processing(let id):
                        guard event.session_id == id else { throw ClientError("Inference error names a different session.") }
                        state = .idle
                    default: throw ClientError("Recoverable error before recording readiness.")
                    }
                }
            default: throw ClientError("Unsupported recording event: \(event.type)")
            }
            sequence += 1
            result.append(event)
        }
        return result
    }

    public func finish(exitStatus: Int32) throws {
        try lines.finish()
        if let errorMessage { throw ClientError(errorMessage) }
        guard exitStatus == 0, state == .idle else { throw ClientError("Recording controller ended before returning to idle (status \(exitStatus)).") }
    }
}
