import { Cube, Desktop, ShieldCheck } from "@phosphor-icons/react";
import { Badge, Theme } from "@radix-ui/themes";

export default function App() {
  return (
    <Theme accentColor="blue" grayColor="slate" radius="small" appearance="inherit">
      <div className="app-shell">
        <header className="titlebar">
          <Cube size={22} weight="fill" />
          <strong>Skill Installer</strong>
          <Badge variant="outline">Windows 0.1.0</Badge>
        </header>
        <main className="welcome">
          <div className="welcome-icon"><Desktop size={30} /></div>
          <h1>Windows 客户端正在初始化</h1>
          <p>独立的 Windows Skill 安装、库存和备份工具。</p>
          <div className="security-note">
            <ShieldCheck size={18} />
            <span>仅在本机处理文件，不执行 Skill 脚本</span>
          </div>
        </main>
      </div>
    </Theme>
  );
}
