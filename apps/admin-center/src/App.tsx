import { Navigate, Route, Routes } from "react-router-dom";
import { PrefsProvider, SessionProvider, useLearnMode, useSession } from "./lib/session";
import ClassicShell from "./components/ClassicShell";
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
import Patches from "./pages/Patches";
import Backups from "./pages/Backups";
import Mobile from "./pages/Mobile";
import NetworkServices from "./pages/NetworkServices";
import RecycleBin from "./pages/RecycleBin";
import PasswordPolicies from "./pages/PasswordPolicies";
import Integrations from "./pages/Integrations";
import Lab, { LiveTopology } from "./pages/Lab";
import ScriptEditor from "./pages/ScriptEditor";
import Audit from "./pages/Audit";
import Learn from "./pages/Learn";
import Settings from "./pages/Settings";
import { UpdatesProvider } from "./lib/updates";
import { BrandingProvider } from "./lib/branding";

function Gate() {
  const { phase } = useSession();
  const { ui } = useLearnMode();
  const Frame = ui === "classic" ? ClassicShell : Shell;
  switch (phase) {
    case "loading": return <div className="grid h-full place-items-center"><Loading label="Starting OpenDirectory…" /></div>;
    case "connect": return <ConnectPage />;
    case "unreachable": return <UnreachablePage />;
    case "setup": return <SetupWizard />;
    case "login": return <LoginPage />;
    case "ready":
      return (
        <Frame>
          <Routes>
            <Route path="/" element={<Overview />} />
            <Route path="/users" element={<Users />} />
            <Route path="/groups" element={<Groups />} />
            <Route path="/ous" element={<OrgUnits />} />
            <Route path="/endpoints" element={<Endpoints />} />
            <Route path="/mobile" element={<Mobile />} />
            <Route path="/endpoints/:id" element={<EndpointDetail />} />
            <Route path="/enroll" element={<Enrollment />} />
            <Route path="/gpos" element={<Policies />} />
            <Route path="/gpos/:id" element={<PolicyEditor />} />
            <Route path="/scripts" element={<Scripts />} />
            <Route path="/patches" element={<Patches />} />
            <Route path="/scripts/:id" element={<ScriptEditor />} />
            <Route path="/audit" element={<Audit />} />
            <Route path="/recycle-bin" element={<RecycleBin />} />
            <Route path="/password-policies" element={<PasswordPolicies />} />
            <Route path="/backups" element={<Backups />} />
            <Route path="/network-services" element={<NetworkServices />} />
            <Route path="/integrations" element={<Integrations />} />
            <Route path="/lab" element={<Lab />} />
            <Route path="/topology" element={<LiveTopology />} />
            <Route path="/learn" element={<Learn />} />
            <Route path="/settings" element={<Settings />} />
            <Route path="*" element={<Navigate to="/" replace />} />
          </Routes>
        </Frame>
      );
  }
}

export default function App() {
  return (
    <PrefsProvider>
      <ToastProvider>
        <UpdatesProvider>
          <SessionProvider>
            <BrandingProvider>
              <Gate />
            </BrandingProvider>
          </SessionProvider>
        </UpdatesProvider>
      </ToastProvider>
    </PrefsProvider>
  );
}
