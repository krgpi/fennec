import AppKit
import SwiftUI

struct AutomationSettingsView: View {
    @ObservedObject private var store = HookStore.shared
    @AppStorage("hookTimeoutSeconds") private var hookTimeoutSeconds = 30

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("イベント発生時に実行するシェルコマンドを設定します。")
                .font(.caption)
                .foregroundStyle(.secondary)

            ScrollView {
                VStack(spacing: 8) {
                    ForEach($store.hooks) { $hook in
                        VStack(alignment: .leading, spacing: 6) {
                            HStack {
                                Picker("", selection: $hook.event) {
                                    ForEach(HookEvent.allCases) { event in
                                        Text(event.displayName).tag(event)
                                    }
                                }
                                .labelsHidden()
                                .fixedSize()
                                Spacer()
                                Toggle("有効", isOn: $hook.enabled)
                                    .toggleStyle(.switch)
                                    .controlSize(.mini)
                                Button {
                                    store.hooks.removeAll { $0.id == hook.id }
                                } label: {
                                    Image(systemName: "trash")
                                }
                                .buttonStyle(.borderless)
                            }
                            TextField("コマンド (例: echo $FENNEC_SESSION_DIR >> ~/fennec.log)", text: $hook.command)
                                .textFieldStyle(.roundedBorder)
                                .font(.system(.caption, design: .monospaced))
                        }
                        .padding(8)
                        .background(Color(nsColor: .controlBackgroundColor))
                        .clipShape(RoundedRectangle(cornerRadius: 6))
                    }

                    Button {
                        store.hooks.append(AutomationHook(event: .recordingStopped, command: ""))
                    } label: {
                        Label("フックを追加", systemImage: "plus")
                    }
                    .frame(maxWidth: .infinity, alignment: .leading)
                }
            }

            Divider()

            HStack {
                Text("タイムアウト")
                TextField("", value: $hookTimeoutSeconds, format: .number)
                    .textFieldStyle(.roundedBorder)
                    .frame(width: 60)
                Text("秒")
                Spacer()
                Button("ログを開く") {
                    NSWorkspace.shared.open(HookRunner.logFileURL)
                }
                .disabled(!FileManager.default.fileExists(atPath: HookRunner.logFileURL.path))
            }

            Text("環境変数: FENNEC_EVENT, FENNEC_SESSION_ID, FENNEC_SESSION_DIR, FENNEC_TRANSCRIPT_FILE, FENNEC_MINUTES_FILE")
                .font(.caption2)
                .foregroundStyle(.secondary)
                .textSelection(.enabled)
        }
    }
}
