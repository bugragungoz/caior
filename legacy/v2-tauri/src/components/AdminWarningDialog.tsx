interface AdminWarningDialogProps {
  packageCount: number;
  onCancel: () => void;
  onContinue: () => void;
}

export function AdminWarningDialog({
  packageCount,
  onCancel,
  onContinue,
}: AdminWarningDialogProps) {
  return (
    <div className="scrim fade-in" onClick={onCancel}>
      <div
        role="dialog"
        aria-modal="true"
        onClick={(e) => e.stopPropagation()}
        style={{
          position: "fixed",
          top: "50%",
          left: "50%",
          transform: "translate(-50%, -50%)",
          width: "420px",
          background: "var(--md-surface-container-high)",
          borderRadius: "28px",
          padding: "24px",
          zIndex: 101,
          display: "flex",
          flexDirection: "column",
          gap: "16px",
        }}
        className="slide-up"
      >
        <div style={{ display: "flex", alignItems: "flex-start", gap: "16px" }}>
          <div
            style={{
              width: "40px",
              height: "40px",
              borderRadius: "50%",
              background: "var(--md-error-container)",
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
              flexShrink: 0,
            }}
          >
            <span
              className="material-symbols-rounded"
              style={{ color: "var(--md-error)", fontSize: "20px" }}
            >
              shield_person
            </span>
          </div>
          <div>
            <p
              style={{
                fontSize: "20px",
                fontWeight: 400,
                color: "var(--md-on-surface)",
                marginBottom: "4px",
              }}
            >
              Administrator Approval Required
            </p>
            <p
              style={{
                fontSize: "14px",
                color: "var(--md-on-surface-variant)",
                lineHeight: "20px",
              }}
            >
              Installing {packageCount} package
              {packageCount !== 1 ? "s" : ""} requires administrator
              privileges. A UAC prompt will appear.
            </p>
          </div>
        </div>

        <div
          style={{
            background: "var(--md-surface-container)",
            borderRadius: "12px",
            padding: "12px 16px",
            display: "flex",
            flexDirection: "column",
            gap: "8px",
          }}
        >
          {[
            "A UAC (User Account Control) prompt will be shown",
            "Click \"Yes\" to allow the operation",
            "If you cancel, the installation will not run",
          ].map((item) => (
            <div
              key={item}
              style={{ display: "flex", alignItems: "flex-start", gap: "10px" }}
            >
              <span
                className="material-symbols-rounded sz-18"
                style={{ color: "var(--md-on-surface-variant)", marginTop: "1px" }}
              >
                info
              </span>
              <p
                style={{
                  fontSize: "13px",
                  color: "var(--md-on-surface-variant)",
                  lineHeight: "18px",
                }}
              >
                {item}
              </p>
            </div>
          ))}
        </div>

        <div
          style={{ display: "flex", justifyContent: "flex-end", gap: "8px" }}
        >
          <button className="btn-text" onClick={onCancel}>
            Cancel
          </button>
          <button className="btn-filled" onClick={onContinue}>
            <span className="material-symbols-rounded sz-18">download</span>
            Continue
          </button>
        </div>
      </div>
    </div>
  );
}
