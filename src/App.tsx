import { useEffect } from "react";
import { Navigate, Route, Routes, useNavigate } from "react-router-dom";

import ConsolePage from "./routes/ConsolePage";
import GuestListPage from "./routes/GuestListPage";
import LoginPage from "./routes/LoginPage";
import SettingsPage from "./routes/SettingsPage";
import { useSession } from "./store/session";

export default function App() {
  const { session, bootstrap, expire } = useSession();
  const navigate = useNavigate();

  useEffect(() => {
    void bootstrap();
  }, [bootstrap]);

  // 会话过期（ticket 真正失效）时统一回登录页，避免各页面各写一遍
  useEffect(() => {
    const onExpired = () => {
      expire();
      navigate("/login", { replace: true });
    };
    window.addEventListener("pve-session-expired", onExpired);
    return () => window.removeEventListener("pve-session-expired", onExpired);
  }, [navigate, expire]);

  return (
    <Routes>
      <Route path="/login" element={<LoginPage />} />
      <Route
        path="/guests"
        element={session ? <GuestListPage /> : <Navigate to="/login" replace />}
      />
      <Route
        path="/console"
        element={session ? <ConsolePage /> : <Navigate to="/login" replace />}
      />
      <Route
        path="/settings"
        element={session ? <SettingsPage /> : <Navigate to="/login" replace />}
      />
      <Route path="*" element={<Navigate to={session ? "/guests" : "/login"} replace />} />
    </Routes>
  );
}
