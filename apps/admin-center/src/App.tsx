import { Navigate, Route, Routes } from "react-router-dom";
import { PrefsProvider, SessionProvider, useSession } from "./lib/session";
import { Loading, ToastProvider } from "./components/ui";
import Shell from "./components/Shell";
import { ConnectPage, LoginPage, SetupWizard, UnreachablePage } from "./pages/Onboarding";
import Overview from "./pages/Overview";
import Users from "./pages/Users";
import Groups from "./pages/Groups";
import OrgUnits from "./pages/OrgUnits";
import Endpoints from "./pages/Endpoints";
import EndpointDetail from "./pages/EndpointDetail";
import Enrollment from "./pages/Enrollment";
import Policies from "./pages/Policies";
import PolicyEditor from "./pages/PolicyEditor";
import Scripts from "./pages/Scripts";
import Backups from "./pages/Backups";
import ScriptEditor from "./pages/ScriptEditor";
import Audit from "./pages/Audit";
import Learn from "./pages/Learn";
import Settings from "./pages/Settings";
import { UpdatesProvider } from "./lib/updates";

function Gate() {
  const { phase } = useSession();
  switch (phase) {
    case "loading": return <div className="grid h-full place-items-center"><Loading label="Starting OpenDirectory…" /></div>;
    case "connect": return <ConnectPage />;
    case "unreachable": return <UnreachablePage />;
    case "setup": return <SetupWizard />;
    case "login": return <LoginPage />;
    case "ready":
      return (
        <Shell>
          <Routes>
            <Route path="/" element={<Overview />} />
            <Route path="/users" element={<Users />} />
            <Route path="/groups" element={<Groups />} />
            <Route path="/ous" element={<OrgUnits />} />
            <Route path="/endpoints" element={<Endpoints />} />
            <Route path="/endpoints/:id" element={<EndpointDetail />} />
            <Route path="/enroll" element={<Enrollment />} />
            <Route path="/gpos" element={<Policies />} />
            <Route path="/gpos/:id" element={<PolicyEditor />} />
            <Route path="/scripts" element={<Scripts />} />
            <Route path="/scripts/:id" element={<ScriptEditor />} />
            <Route path="/audit" element={<Audit />} />
            <Route path="/backups" element={<Backups />} />
            <Route path="/learn" element={<Learn />} />
            <Route path="/settings" element={<Settings />} />
            <Route path="*" element={<Navigate to="/" replace />} />
          </Routes>
        </Shell>
      );
  }
}

export default function App() {
  return (
    <PrefsProvider>
      <ToastProvider>
        <UpdatesProvider>
          <SessionProvider>
            <Gate />
          </SessionProvider>
        </UpdatesProvider>
      </ToastProvider>
    </PrefsProvider>
  );
}
