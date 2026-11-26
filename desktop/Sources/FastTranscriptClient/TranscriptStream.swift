import Foundation

public struct TranscriptSnapshot: Decodable, Equatable, Sendable {
    public let text: String
    public let segments: [TranscriptSegment]?
}

public struct TranscriptSegment: Decodable, Equatable, Sendable {
    public let start_s: Double
    public let end_s: Double
    public let text: String
    public let speaker: String?
}

public struct TranscriptEvent: Decodable, Sendable {
    public let version: Int
    public let job_id: String
    public let sequence: UInt64
    public let type: String
    public let data: Payload

    public struct Payload: Decodable, Sendable {
        public let text: String?
        public let segments: [TranscriptSegment]?
        public let result: TranscriptSnapshot?
        public let output_path: String?
        public let message: String?
    }
}

public struct ClientError: LocalizedError, Sendable {
    public let message: String
    public init(_ message: String) { self.message = message }
    public var errorDescription: String? { message }
}

/// Presentation-only decoder. Transcription, merging, and exports stay in Rust.
public struct TranscriptStreamDecoder {
    private var pending = Data()
    private var jobID: String?
    private var sequence: UInt64 = 0
    private var terminal = false
    public private(set) var completed = false
    public private(set) var errorMessage: String?

    public init() {}

    public mutating func append(_ data: Data) throws -> [TranscriptEvent] {
        pending.append(data)
        var events: [TranscriptEvent] = []
        while let newline = pending.firstIndex(of: 10) {
            let line = pending[..<newline]
            pending.removeSubrange(...newline)
            if line.allSatisfy({ [UInt8(9), 13, 32].contains($0) }) { continue }
            guard !terminal else { throw ClientError("Received data after a terminal event.") }
            let event = try JSONDecoder().decode(TranscriptEvent.self, from: Data(line))
            guard event.version == 1, !event.job_id.isEmpty, event.sequence == sequence else {
                throw ClientError("Unsupported or out-of-order transcription event.")
            }
            if let jobID {
                guard event.job_id == jobID else { throw ClientError("Transcription stream mixed different jobs.") }
            } else {
                guard event.type == "started" else { throw ClientError("Transcription did not begin with started.") }
                jobID = event.job_id
            }
            switch event.type {
            case "started" where sequence == 0, "ready": break
            case "update":
                guard event.data.text != nil else { throw ClientError("Transcription update is missing text.") }
            case "completed":
                guard event.data.result != nil else { throw ClientError("Completed transcription is missing its result.") }
                terminal = true
                completed = true
            case "error":
                terminal = true
                errorMessage = event.data.message ?? "Transcription failed."
            default: throw ClientError("Unsupported transcription event: \(event.type)")
            }
            guard sequence < UInt64.max else { throw ClientError("Transcription sequence overflow.") }
            sequence += 1
            events.append(event)
        }
        return events
    }

    public func finish(exitStatus: Int32) throws {
        guard pending.allSatisfy({ [UInt8(9), 10, 13, 32].contains($0) }) else {
            throw ClientError("Transcription ended with an incomplete event.")
        }
        if let errorMessage { throw ClientError(errorMessage) }
        guard exitStatus == 0 else { throw ClientError("Transcription process exited with status \(exitStatus).") }
        guard completed else { throw ClientError("Transcription ended before completion.") }
    }
}
