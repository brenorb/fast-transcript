import SwiftUI

@MainActor
struct CalendarAccountsView: View {
    let workspace: Workspace
    @Environment(\.dismiss) private var dismiss
    @State private var state = LoadState.loading

    private enum LoadState {
        case loading
        case loaded(CalendarStatus)
        case failed(String)
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text("Calendar accounts").font(.title2.bold())
            Text("Google Calendar works here when its account is added in System Settings → Internet Accounts with Calendars enabled. You can keep using Google Calendar in your browser.")
                .foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
            content
            HStack {
                Button("Refresh") { Task { await load() } }
                    .disabled(isLoading)
                    .accessibilityIdentifier("refreshCalendarAccounts")
                Spacer()
                Button("Done") { dismiss() }
                    .keyboardShortcut(.defaultAction)
                    .accessibilityIdentifier("closeCalendarAccounts")
            }
        }
        .padding(24)
        .frame(width: 520)
        .task { await load() }
    }

    @ViewBuilder private var content: some View {
        switch state {
        case .loading:
            ProgressView("Reading calendars on this Mac…").frame(maxWidth: .infinity, minHeight: 120)
        case .failed(let message):
            Label(message, systemImage: "exclamationmark.circle").textSelection(.enabled)
        case .loaded(let result):
            Text("Calendar access: \(result.authorization.replacingOccurrences(of: "_", with: " "))")
                .accessibilityIdentifier("calendarAccountsAccess")
            if !result.queried {
                Text(["denied", "restricted"].contains(result.authorization)
                     ? "Calendar accounts have not been read. Allow Fast Transcript in System Settings → Privacy & Security → Calendars, then refresh here."
                     : "Calendar accounts have not been read. Enable Use calendar meeting names to allow access, then refresh here.")
                    .foregroundStyle(.secondary)
            } else if result.calendars.isEmpty {
                Text("No calendars are visible on this Mac. Check that Calendars is enabled for your account in Internet Accounts.")
                    .foregroundStyle(.secondary)
            } else {
                Text("Calendars visible to Fast Transcript").font(.headline)
                ScrollView {
                    LazyVStack(alignment: .leading, spacing: 12) {
                        ForEach(result.calendars) { calendar in
                            VStack(alignment: .leading, spacing: 2) {
                                Text(calendar.title.isEmpty ? "Untitled calendar" : calendar.title)
                                if !calendar.source.isEmpty { Text(calendar.source).font(.caption).foregroundStyle(.secondary) }
                            }
                            .frame(maxWidth: .infinity, alignment: .leading)
                            .accessibilityElement(children: .combine)
                        }
                    }
                }
                .frame(height: 180)
                .accessibilityIdentifier("visibleCalendars")
                Text("This is the local macOS inventory. It does not prove that a remote account has finished syncing.")
                    .font(.caption).foregroundStyle(.secondary)
            }
        }
    }

    private var isLoading: Bool { if case .loading = state { true } else { false } }

    private func load() async {
        state = .loading
        do {
            let result = try await workspace.readCalendarAccounts()
            guard !Task.isCancelled else { return }
            state = .loaded(result)
        } catch is CancellationError {
            return
        } catch {
            guard !Task.isCancelled else { return }
            state = .failed(error.localizedDescription)
        }
    }
}
