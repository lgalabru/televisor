import SwiftUI

struct SettingsView: View {
    @Environment(AppModel.self) private var model
    @State private var isRefreshing = false
    @State private var isConfirmingSignOut = false
    @State private var regions: [Region] = []

    var body: some View {
        content
            .task { await model.refreshStatus() }
            .task {
                guard let categories = try? await Televisor.categories(.live) else { return }
                regions = RegionGroups(categories, preferred: model.region).groups
                    .map(\.region)
                    .filter { $0.code != "Other" }
            }
            .confirmationDialog("Sign out of this provider?", isPresented: $isConfirmingSignOut) {
                Button("Sign Out", role: .destructive) {
                    Task { await model.signOut() }
                }
            }
    }

    #if os(iOS)
    private var content: some View {
        Form {
            if case .signedIn(let overview) = model.phase {
                Section("Subscription") { accountRows(overview) }
            }
            Section {
                regionPicker
            } footer: {
                Text("Its channels come first in Live TV, on Home, and among a game’s feeds.")
            }
            Section {
                refreshButton
                signOutButton
            } footer: {
                Text(refreshNote)
            }
            Section {
                LabeledContent("Version", value: versionLine)
            }
        }
        .navigationTitle("Settings")
    }
    #else
    private var content: some View {
        HStack(alignment: .top, spacing: 120) {
            VStack(alignment: .leading, spacing: 30) {
                Wordmark()
                Text(versionLine).font(.caption).foregroundStyle(.tertiary)
            }
            .frame(width: 560, alignment: .leading)

            VStack(alignment: .leading, spacing: 40) {
                if case .signedIn(let overview) = model.phase {
                    account(overview)
                }
                regionPicker
                VStack(spacing: 20) {
                    refreshButton
                    signOutButton
                }
                Text(refreshNote)
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            .frame(maxWidth: 900)
        }
        .padding(80)
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
    }
    #endif

    private var refreshNote: String {
        "Channel, movie and series lists are kept for a day. Refresh to see what your provider added since."
    }

    private var refreshButton: some View {
        Button {
            isRefreshing = true
            Task {
                await model.refreshLibrary()
                isRefreshing = false
            }
        } label: {
            HStack {
                Label("Refresh Library", systemImage: "arrow.clockwise")
                Spacer()
                if isRefreshing { ProgressView() }
            }
        }
        .disabled(isRefreshing)
    }

    private var signOutButton: some View {
        Button(role: .destructive) {
            isConfirmingSignOut = true
        } label: {
            Label("Sign Out", systemImage: "rectangle.portrait.and.arrow.right")
                .frame(maxWidth: .infinity, alignment: .leading)
        }
    }

    /// Which region's channels come first in Live TV, on Home, and among a
    /// game's feeds.
    @ViewBuilder private var regionPicker: some View {
        @Bindable var model = model
        if !regions.isEmpty {
            Picker(selection: $model.region) {
                ForEach(regions) { region in
                    Text("\(region.flag)  \(region.name)").tag(region.code)
                }
            } label: {
                Label("Preferred Region", systemImage: "globe")
            }
            .pickerStyle(.menu)
        }
    }

    private func account(_ overview: AccountOverview) -> some View {
        VStack(alignment: .leading, spacing: 18) {
            Text("Subscription").font(.headline)
            accountRows(overview)
        }
        .padding(40)
        .background(Theme.raised, in: RoundedRectangle(cornerRadius: 24, style: .continuous))
    }

    @ViewBuilder
    private func accountRows(_ overview: AccountOverview) -> some View {
            row("Status", overview.status ?? "Unknown")
            row("Renews or Expires", overview.expiresAt.map {
                Date(timeIntervalSince1970: TimeInterval($0)).formatted(date: .long, time: .omitted)
            } ?? "Never")
            if let max = overview.connectionsMax {
                row("Screens in Use", "\(overview.connectionsActive ?? 0) of \(max)")
            }
            if overview.isTrial {
                row("Plan", "Trial")
            }
            if let host = model.host {
                row("Server", host)
            }
            if let timezone = overview.timezone {
                row("Provider Time Zone", timezone)
            }
    }

    private func row(_ label: String, _ value: String) -> some View {
        HStack {
            Text(label).foregroundStyle(.secondary)
            Spacer(minLength: 40)
            Text(value).lineLimit(1)
        }
        .font(.callout)
    }

    private var versionLine: String {
        let info = Bundle.main.infoDictionary
        let version = info?["CFBundleShortVersionString"] as? String ?? "?"
        let build = info?["CFBundleVersion"] as? String ?? "?"
        return "Version \(version) (\(build))"
    }
}
