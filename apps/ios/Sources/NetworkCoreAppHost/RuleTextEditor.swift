import NetworkCoreBridge
import SwiftUI
#if canImport(UIKit)
import UIKit
#else
import AppKit
#endif

/// The user's local rules as one text (`RuleText`): colored by field as you type, every bad line underlined and
/// listed under the editor, and a tap on a listed problem puts the cursor on that line. Saving is refused while a
/// line is wrong. A running tunnel takes the saved rules in place when only rule lines changed, so open
/// connections keep going; a new rule-list URL still reconnects it.
///
/// Unsaved changes are not lost: with changes, the back button asks to save or discard them (and the swipe back is
/// off), and the text is kept for the scene, so leaving another way (the Mac sidebar, the app being closed) offers it
/// again next time while the saved rules are unchanged. The lines are checked a moment after typing stops, away from
/// the main thread, and a keystroke recolors only the lines it changed.
struct RuleTextEditorView: View {
    var tunnel: TunnelController
    @Environment(\.brand) private var brand
    @Environment(\.dismiss) private var dismiss
    @State private var text = ""
    @State private var original = ""
    @State private var issues: [RuleText.Issue] = []
    /// Bumped each time `issues` is set, so the text view colors the whole text again.
    @State private var checkRevision = 0
    @State private var jump: NSRange?
    @State private var loaded = false
    @State private var saved: Date?
    @State private var restored = false
    @State private var confirmingLeave = false
    /// The unsaved text and the saved rules' text it was written over.
    @SceneStorage("rules.textEditor.draft") private var draft = ""
    @SceneStorage("rules.textEditor.draftBase") private var draftBase = ""

    private var changed: Bool {
        text != original
    }

    var body: some View {
        VStack(spacing: 0) {
            HighlightedRuleText(
                text: $text,
                issues: issues,
                revision: checkRevision,
                jump: $jump,
                palette: RuleTextColors(brand: brand)
            )
            .frame(minHeight: 220)
            .background(brand.surface)
            Divider()
            issueList
        }
        .background(brand.bg.ignoresSafeArea())
        .navigationTitle("编辑规则文本")
        .inlineNavigationTitle()
        // With unsaved changes the system back button (and on iOS the swipe back with it) gives way to one that asks.
        .navigationBarBackButtonHidden(changed)
        .toolbar {
            if changed {
                ToolbarItem(placement: .navigation) {
                    Button {
                        confirmingLeave = true
                    } label: {
                        Label("返回", systemImage: "chevron.backward")
                            .labelStyle(.titleAndIcon)
                    }
                }
            }
            ToolbarItem(placement: .confirmationAction) {
                Button("保存") {
                    save()
                }
                .disabled(!changed || !issues.isEmpty)
            }
        }
        .confirmationDialog("保存对规则的修改？", isPresented: $confirmingLeave, titleVisibility: .visible) {
            if issues.isEmpty {
                Button("保存") {
                    save()
                    if !changed {
                        dismiss()
                    }
                }
            }
            Button("放弃修改", role: .destructive) {
                discard()
                dismiss()
            }
            Button("继续编辑", role: .cancel) {}
        } message: {
            if issues.isEmpty {
                Text("离开后，未保存的修改会丢失。")
            } else {
                Text("还有错误没有改好，现在不能保存。离开后，未保存的修改会丢失。")
            }
        }
        .onAppear {
            guard !loaded else {
                return
            }
            loaded = true
            let current = RuleText.text(of: tunnel.localRules)
            original = current
            if let kept = RuleText.restorableDraft(draft, base: draftBase, current: current) {
                text = kept
                restored = true
            } else {
                text = current
                keepDraft()
            }
        }
        .task(id: text) {
            // Checked once typing pauses, off the main thread; saving checks again, so a stale result cannot let a
            // bad line through.
            try? await Task.sleep(for: .milliseconds(200))
            guard !Task.isCancelled else {
                return
            }
            let current = text
            let found = await Task.detached(priority: .userInitiated) {
                RuleText.issues(in: current)
            }.value
            guard !Task.isCancelled, current == text else {
                return
            }
            setIssues(found)
            keepDraft()
        }
    }

    private func setIssues(_ next: [RuleText.Issue]) {
        issues = next
        checkRevision &+= 1
    }

    /// Keeps the unsaved text for the scene, or forgets it when there is none.
    private func keepDraft() {
        if changed {
            draft = text
            draftBase = original
        } else {
            draft = ""
            draftBase = ""
        }
    }

    private func discard() {
        text = original
        restored = false
        draft = ""
        draftBase = ""
    }

    @ViewBuilder
    private var issueList: some View {
        if issues.isEmpty {
            VStack(alignment: .leading, spacing: 4) {
                Label(statusTitle, systemImage: "checkmark.circle")
                    .font(.footnote.weight(.semibold))
                    .foregroundStyle(brand.text2)
                Text("一行一条，写法同 Surge / Loon：类型,值,策略[,no-resolve]。策略写 DIRECT、REJECT，其他名字都当作代理。以 # 开头的整条规则表示停用，其余注释保存时不保留。连接中保存会直接在隧道里换上新规则，已有连接不断开；新增规则列表 URL 时会重新连接。")
                    .font(.caption)
                    .foregroundStyle(brand.text2)
                    .fixedSize(horizontal: false, vertical: true)
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(12)
        } else {
            List {
                Section {
                    ForEach(issues) { issue in
                        Button {
                            jump = issue.range
                        } label: {
                            HStack(alignment: .firstTextBaseline, spacing: 8) {
                                Text("第 \(issue.line + 1) 行")
                                    .font(.footnote.monospacedDigit().weight(.semibold))
                                    .foregroundStyle(brand.danger)
                                Text(Self.message(issue.problem))
                                    .font(.footnote)
                                    .foregroundStyle(brand.text)
                                Spacer(minLength: 0)
                                Image(systemName: "arrow.turn.down.right")
                                    .font(.caption)
                                    .foregroundStyle(brand.text2)
                            }
                            .contentShape(Rectangle())
                        }
                        .buttonStyle(.plain)
                        .listRowBackground(brand.surface)
                    }
                } header: {
                    Text("\(issues.count) 处错误，改好后才能保存")
                }
            }
            .scrollContentBackground(.hidden)
            .frame(maxHeight: 200)
        }
    }

    private var statusTitle: String {
        if restored, changed {
            return String(localized: "已恢复上次未保存的修改")
        }
        if let saved, tunnel.lastRulesReload.map({ $0 >= saved }) == true {
            return String(localized: "已保存，隧道已热重载规则")
        }
        if saved != nil {
            return String(localized: "已保存")
        }
        return String(localized: "没有错误")
    }

    private func save() {
        do {
            let next = try RuleText.apply(text, to: tunnel.localRules)
            saved = Date()
            tunnel.updateLocalRules(next)
            text = RuleText.text(of: next)
            original = text
            restored = false
            keepDraft()
        } catch let rejected as RuleText.Rejected {
            setIssues(rejected.issues)
        } catch {
            setIssues(RuleText.issues(in: text))
        }
    }

    static func message(_ problem: RuleText.Problem) -> String {
        switch problem {
        case let .unsupportedKind(kind):
            return String(localized: "不支持的规则类型 \(kind)")
        case .missingValue:
            return String(localized: "缺少规则的值")
        case .invalidValue:
            return String(localized: "值的写法不对，内核不会接受")
        case .final:
            return String(localized: "FINAL 由预设或订阅决定，这里不能写")
        case .tooMany:
            return String(localized: "最多 \(LocalRules.maxRules) 条规则")
        }
    }
}

/// The editor's colors, taken from the brand palette so both themes read well.
struct RuleTextColors {
    let text: Color
    let kind: Color
    let value: Color
    let policy: Color
    let option: Color
    let comment: Color
    let error: Color

    init(brand: BrandPalette) {
        text = brand.text
        kind = brand.primary
        value = brand.text
        policy = brand.amberText
        option = brand.text2
        comment = brand.text2
        error = brand.danger
    }

    #if canImport(UIKit)
    typealias PlatformColor = UIColor
    typealias PlatformFont = UIFont
    #else
    typealias PlatformColor = NSColor
    typealias PlatformFont = NSFont
    #endif

    func color(_ kind: RuleText.TokenKind) -> PlatformColor {
        switch kind {
        case .comment:
            return PlatformColor(comment)
        case .kind:
            return PlatformColor(self.kind)
        case .value:
            return PlatformColor(value)
        case .policy:
            return PlatformColor(policy)
        case .option, .separator:
            return PlatformColor(option)
        }
    }

    static var font: PlatformFont {
        #if canImport(UIKit)
        PlatformFont.monospacedSystemFont(ofSize: 14, weight: .regular)
        #else
        PlatformFont.monospacedSystemFont(ofSize: 13, weight: .regular)
        #endif
    }

    static var boldFont: PlatformFont {
        #if canImport(UIKit)
        PlatformFont.monospacedSystemFont(ofSize: 14, weight: .semibold)
        #else
        PlatformFont.monospacedSystemFont(ofSize: 13, weight: .semibold)
        #endif
    }

    /// Colors `storage` in place: base attributes, then each token, then a red underline under every issue.
    /// Attributes only; the characters and the selection stay.
    func paint(_ storage: NSTextStorage, issues: [RuleText.Issue]) {
        let text = storage.string
        let whole = NSRange(location: 0, length: storage.length)
        storage.beginEditing()
        storage.setAttributes([.font: Self.font, .foregroundColor: PlatformColor(self.text)], range: whole)
        for token in RuleText.tokens(in: text) where NSMaxRange(token.range) <= storage.length {
            storage.addAttribute(.foregroundColor, value: color(token.kind), range: token.range)
            if token.kind == .kind {
                storage.addAttribute(.font, value: Self.boldFont, range: token.range)
            }
        }
        let red = PlatformColor(error)
        for issue in issues where NSMaxRange(issue.range) <= storage.length {
            storage.addAttributes([
                .underlineStyle: NSUnderlineStyle.thick.rawValue | NSUnderlineStyle.patternDot.rawValue,
                .underlineColor: red,
                .backgroundColor: red.withAlphaComponent(0.12),
            ], range: issue.range)
        }
        storage.endEditing()
    }

    /// Colors only `lines` of `storage` (whole lines, as `RuleText.changedLines` gives them) after a keystroke. Their
    /// issue marks go until the next check marks them again; the other lines keep theirs, which move with the text.
    func paint(_ storage: NSTextStorage, lines: NSRange) {
        let range = NSIntersectionRange(lines, NSRange(location: 0, length: storage.length))
        guard range.length > 0 else {
            return
        }
        storage.beginEditing()
        storage.setAttributes([.font: Self.font, .foregroundColor: PlatformColor(self.text)], range: range)
        for token in RuleText.tokens(in: storage.string, lines: range) where NSMaxRange(token.range) <= NSMaxRange(range) {
            storage.addAttribute(.foregroundColor, value: color(token.kind), range: token.range)
            if token.kind == .kind {
                storage.addAttribute(.font, value: Self.boldFont, range: token.range)
            }
        }
        storage.endEditing()
    }

    /// Colors `storage`: all of it when `revision` is newer than the coordinator last painted (new issues, or text
    /// set from outside), else only the lines changed since the last paint.
    func update(_ storage: NSTextStorage, issues: [RuleText.Issue], revision: Int, painted: inout PaintState) {
        let current = storage.string
        if painted.revision != revision || painted.text == nil {
            paint(storage, issues: issues)
        } else if let previous = painted.text, let lines = RuleText.changedLines(from: previous, to: current) {
            paint(storage, lines: lines)
        }
        painted = PaintState(revision: revision, text: current)
    }

    /// What a text view was last colored for.
    struct PaintState {
        var revision: Int?
        var text: String?
    }
}

#if canImport(UIKit)
/// A plain-text `UITextView` whose characters are colored by `RuleTextColors`; setting `jump` selects that range,
/// scrolls to it and focuses the view.
struct HighlightedRuleText: UIViewRepresentable {
    @Binding var text: String
    let issues: [RuleText.Issue]
    /// Bumped when `issues` were worked out again; the whole text is colored then.
    let revision: Int
    @Binding var jump: NSRange?
    let palette: RuleTextColors

    func makeCoordinator() -> Coordinator {
        Coordinator(text: $text)
    }

    func makeUIView(context: Context) -> UITextView {
        let view = UITextView()
        view.delegate = context.coordinator
        view.backgroundColor = .clear
        view.autocapitalizationType = .none
        view.autocorrectionType = .no
        view.smartQuotesType = .no
        view.smartDashesType = .no
        view.spellCheckingType = .no
        view.keyboardType = .asciiCapable
        view.textContainerInset = UIEdgeInsets(top: 12, left: 8, bottom: 12, right: 8)
        view.alwaysBounceVertical = true
        view.text = text
        return view
    }

    func updateUIView(_ view: UITextView, context: Context) {
        context.coordinator.text = $text
        if view.text != text {
            view.text = text
            // Set from outside: the old attributes are gone, so everything is colored again.
            context.coordinator.painted = RuleTextColors.PaintState()
        }
        palette.update(view.textStorage, issues: issues, revision: revision, painted: &context.coordinator.painted)
        view.typingAttributes = [.font: RuleTextColors.font, .foregroundColor: UIColor(palette.text)]
        if let range = jump, NSMaxRange(range) <= (view.text as NSString).length {
            DispatchQueue.main.async {
                view.becomeFirstResponder()
                view.selectedRange = range
                view.scrollRangeToVisible(range)
                jump = nil
            }
        }
    }

    final class Coordinator: NSObject, UITextViewDelegate {
        var text: Binding<String>
        var painted = RuleTextColors.PaintState()

        init(text: Binding<String>) {
            self.text = text
        }

        func textViewDidChange(_ textView: UITextView) {
            text.wrappedValue = textView.text
        }
    }
}
#else
/// A plain-text `NSTextView` whose characters are colored by `RuleTextColors`; setting `jump` selects that range,
/// scrolls to it and focuses the view.
struct HighlightedRuleText: NSViewRepresentable {
    @Binding var text: String
    let issues: [RuleText.Issue]
    /// Bumped when `issues` were worked out again; the whole text is colored then.
    let revision: Int
    @Binding var jump: NSRange?
    let palette: RuleTextColors

    func makeCoordinator() -> Coordinator {
        Coordinator(text: $text)
    }

    func makeNSView(context: Context) -> NSScrollView {
        let scroll = NSTextView.scrollableTextView()
        scroll.drawsBackground = false
        if let view = scroll.documentView as? NSTextView {
            view.delegate = context.coordinator
            view.isRichText = false
            view.allowsUndo = true
            view.drawsBackground = false
            view.isAutomaticQuoteSubstitutionEnabled = false
            view.isAutomaticDashSubstitutionEnabled = false
            view.isAutomaticTextReplacementEnabled = false
            view.isAutomaticSpellingCorrectionEnabled = false
            view.isContinuousSpellCheckingEnabled = false
            view.textContainerInset = NSSize(width: 8, height: 12)
            view.string = text
        }
        return scroll
    }

    func updateNSView(_ scroll: NSScrollView, context: Context) {
        context.coordinator.text = $text
        guard let view = scroll.documentView as? NSTextView, let storage = view.textStorage else {
            return
        }
        if view.string != text {
            view.string = text
            // Set from outside: the old attributes are gone, so everything is colored again.
            context.coordinator.painted = RuleTextColors.PaintState()
        }
        palette.update(storage, issues: issues, revision: revision, painted: &context.coordinator.painted)
        view.typingAttributes = [.font: RuleTextColors.font, .foregroundColor: NSColor(palette.text)]
        if let range = jump, NSMaxRange(range) <= (view.string as NSString).length {
            DispatchQueue.main.async {
                view.window?.makeFirstResponder(view)
                view.setSelectedRange(range)
                view.scrollRangeToVisible(range)
                view.showFindIndicator(for: range)
                jump = nil
            }
        }
    }

    final class Coordinator: NSObject, NSTextViewDelegate {
        var text: Binding<String>
        var painted = RuleTextColors.PaintState()

        init(text: Binding<String>) {
            self.text = text
        }

        func textDidChange(_ notification: Notification) {
            guard let view = notification.object as? NSTextView else {
                return
            }
            text.wrappedValue = view.string
        }
    }
}
#endif
