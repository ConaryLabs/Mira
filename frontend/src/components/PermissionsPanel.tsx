// frontend/src/components/PermissionsPanel.tsx
// Permissions management panel for sudo commands and filesystem access

import React, { useEffect, useState } from 'react';
import { Shield, Plus, Trash2, ToggleLeft, ToggleRight, ChevronDown, ChevronRight, Folder, Lock, Unlock, AlertTriangle, RefreshCw } from 'lucide-react';
import { useSudoStore, SudoPermission, SudoBlocklistEntry } from '../stores/useSudoStore';
import { useAppState } from '../stores/useAppState';

type SubTab = 'permissions' | 'blocklist' | 'access';

export function PermissionsPanel() {
  const [subTab, setSubTab] = useState<SubTab>('access');
  const [expandedSections, setExpandedSections] = useState<Set<string>>(new Set(['filesystem']));
  const [showAddPermission, setShowAddPermission] = useState(false);
  const [newPermission, setNewPermission] = useState({
    name: '',
    description: '',
    command_prefix: '',
    requires_approval: true,
  });

  const {
    permissions,
    blocklist,
    loading,
    fetchPermissions,
    fetchBlocklist,
    addPermission,
    removePermission,
    togglePermission,
  } = useSudoStore();

  const { systemAccessMode, setSystemAccessMode } = useAppState();

  // Fetch permissions and blocklist on mount
  useEffect(() => {
    fetchPermissions();
    fetchBlocklist();
  }, [fetchPermissions, fetchBlocklist]);

  const toggleSection = (section: string) => {
    const next = new Set(expandedSections);
    if (next.has(section)) {
      next.delete(section);
    } else {
      next.add(section);
    }
    setExpandedSections(next);
  };

  const handleAddPermission = async () => {
    if (!newPermission.name || !newPermission.command_prefix) return;

    await addPermission({
      name: newPermission.name,
      description: newPermission.description || undefined,
      command_prefix: newPermission.command_prefix,
      requires_approval: newPermission.requires_approval,
    });

    setNewPermission({ name: '', description: '', command_prefix: '', requires_approval: true });
    setShowAddPermission(false);
    fetchPermissions();
  };

  const renderAccessTab = () => (
    <div className="p-4 space-y-4">
      {/* Filesystem Access Section */}
      <div className="border border-gray-200 dark:border-slate-700 rounded-lg overflow-hidden">
        <button
          onClick={() => toggleSection('filesystem')}
          className="w-full flex items-center justify-between px-4 py-3 bg-gray-50 dark:bg-slate-800/50 hover:bg-gray-100 dark:hover:bg-slate-800 transition-colors"
        >
          <div className="flex items-center gap-2">
            <Folder className="w-4 h-4 text-blue-500" />
            <span className="font-medium text-sm text-gray-800 dark:text-slate-200">Filesystem Access</span>
          </div>
          {expandedSections.has('filesystem') ? (
            <ChevronDown className="w-4 h-4 text-gray-400" />
          ) : (
            <ChevronRight className="w-4 h-4 text-gray-400" />
          )}
        </button>

        {expandedSections.has('filesystem') && (
          <div className="p-4 space-y-3">
            <p className="text-xs text-gray-500 dark:text-slate-400">
              Control which directories Mira can access with project tools (read_project_file, search_codebase, etc.)
            </p>

            {/* Access Mode Toggle */}
            <div className="space-y-2">
              <button
                onClick={() => setSystemAccessMode('project')}
                className={`w-full flex items-center gap-3 p-3 rounded-lg border transition-colors ${
                  systemAccessMode === 'project'
                    ? 'border-blue-500 bg-blue-50 dark:bg-blue-900/20'
                    : 'border-gray-200 dark:border-slate-700 hover:border-gray-300 dark:hover:border-slate-600'
                }`}
              >
                <Lock className={`w-5 h-5 ${systemAccessMode === 'project' ? 'text-blue-500' : 'text-gray-400'}`} />
                <div className="text-left flex-1">
                  <div className="text-sm font-medium text-gray-800 dark:text-slate-200">Project Only</div>
                  <div className="text-xs text-gray-500 dark:text-slate-400">
                    Access limited to current project directory
                  </div>
                </div>
                {systemAccessMode === 'project' && (
                  <div className="w-2 h-2 rounded-full bg-blue-500" />
                )}
              </button>

              <button
                onClick={() => setSystemAccessMode('home')}
                className={`w-full flex items-center gap-3 p-3 rounded-lg border transition-colors ${
                  systemAccessMode === 'home'
                    ? 'border-yellow-500 bg-yellow-50 dark:bg-yellow-900/20'
                    : 'border-gray-200 dark:border-slate-700 hover:border-gray-300 dark:hover:border-slate-600'
                }`}
              >
                <Unlock className={`w-5 h-5 ${systemAccessMode === 'home' ? 'text-yellow-500' : 'text-gray-400'}`} />
                <div className="text-left flex-1">
                  <div className="text-sm font-medium text-gray-800 dark:text-slate-200">Home Directory</div>
                  <div className="text-xs text-gray-500 dark:text-slate-400">
                    Access to ~/home and subdirectories
                  </div>
                </div>
                {systemAccessMode === 'home' && (
                  <div className="w-2 h-2 rounded-full bg-yellow-500" />
                )}
              </button>

              <button
                onClick={() => setSystemAccessMode('system')}
                className={`w-full flex items-center gap-3 p-3 rounded-lg border transition-colors ${
                  systemAccessMode === 'system'
                    ? 'border-red-500 bg-red-50 dark:bg-red-900/20'
                    : 'border-gray-200 dark:border-slate-700 hover:border-gray-300 dark:hover:border-slate-600'
                }`}
              >
                <AlertTriangle className={`w-5 h-5 ${systemAccessMode === 'system' ? 'text-red-500' : 'text-gray-400'}`} />
                <div className="text-left flex-1">
                  <div className="text-sm font-medium text-gray-800 dark:text-slate-200">Full System</div>
                  <div className="text-xs text-gray-500 dark:text-slate-400">
                    Access to entire filesystem (use with caution)
                  </div>
                </div>
                {systemAccessMode === 'system' && (
                  <div className="w-2 h-2 rounded-full bg-red-500" />
                )}
              </button>
            </div>

            {systemAccessMode !== 'project' && (
              <div className="flex items-start gap-2 p-3 bg-yellow-50 dark:bg-yellow-900/20 border border-yellow-200 dark:border-yellow-800 rounded-lg">
                <AlertTriangle className="w-4 h-4 text-yellow-600 dark:text-yellow-400 mt-0.5 flex-shrink-0" />
                <div className="text-xs text-yellow-700 dark:text-yellow-300">
                  Extended access is temporary and will reset when you close this session.
                  Mira will be able to read files outside the project using project tools.
                </div>
              </div>
            )}
          </div>
        )}
      </div>

      {/* Sudo Commands Section */}
      <div className="border border-gray-200 dark:border-slate-700 rounded-lg overflow-hidden">
        <button
          onClick={() => toggleSection('sudo')}
          className="w-full flex items-center justify-between px-4 py-3 bg-gray-50 dark:bg-slate-800/50 hover:bg-gray-100 dark:hover:bg-slate-800 transition-colors"
        >
          <div className="flex items-center gap-2">
            <Shield className="w-4 h-4 text-purple-500" />
            <span className="font-medium text-sm text-gray-800 dark:text-slate-200">Sudo Commands</span>
            <span className="text-xs text-gray-400 dark:text-slate-500">
              ({permissions.filter(p => p.enabled).length} active)
            </span>
          </div>
          {expandedSections.has('sudo') ? (
            <ChevronDown className="w-4 h-4 text-gray-400" />
          ) : (
            <ChevronRight className="w-4 h-4 text-gray-400" />
          )}
        </button>

        {expandedSections.has('sudo') && (
          <div className="p-4 space-y-3">
            <p className="text-xs text-gray-500 dark:text-slate-400">
              Commands that require sudo access. Some auto-approve, others need your confirmation.
            </p>

            <div className="space-y-2 max-h-48 overflow-y-auto">
              {permissions.map((perm) => (
                <div
                  key={perm.id}
                  className={`flex items-center justify-between p-2 rounded border ${
                    perm.enabled
                      ? 'border-gray-200 dark:border-slate-700 bg-white dark:bg-slate-800'
                      : 'border-gray-100 dark:border-slate-800 bg-gray-50 dark:bg-slate-850 opacity-60'
                  }`}
                >
                  <div className="flex-1 min-w-0">
                    <div className="text-sm font-medium text-gray-800 dark:text-slate-200 truncate">
                      {perm.name}
                    </div>
                    <div className="text-xs text-gray-500 dark:text-slate-400 truncate">
                      {perm.command_prefix || perm.command_pattern || perm.command_exact}
                    </div>
                  </div>
                  <div className="flex items-center gap-2 ml-2">
                    {perm.requires_approval && (
                      <span className="text-xs px-1.5 py-0.5 bg-yellow-100 dark:bg-yellow-900/30 text-yellow-700 dark:text-yellow-400 rounded">
                        approval
                      </span>
                    )}
                    <button
                      onClick={() => togglePermission(perm.id, !perm.enabled)}
                      className="p-1 hover:bg-gray-100 dark:hover:bg-slate-700 rounded"
                      title={perm.enabled ? 'Disable' : 'Enable'}
                    >
                      {perm.enabled ? (
                        <ToggleRight className="w-5 h-5 text-green-500" />
                      ) : (
                        <ToggleLeft className="w-5 h-5 text-gray-400" />
                      )}
                    </button>
                  </div>
                </div>
              ))}
            </div>

            <button
              onClick={() => setSubTab('permissions')}
              className="w-full text-center text-xs text-blue-600 dark:text-blue-400 hover:underline"
            >
              Manage all permissions...
            </button>
          </div>
        )}
      </div>
    </div>
  );

  const renderPermissionsTab = () => (
    <div className="p-4 space-y-4">
      <div className="flex items-center justify-between">
        <h3 className="font-medium text-sm text-gray-800 dark:text-slate-200">Sudo Permissions</h3>
        <div className="flex items-center gap-2">
          <button
            onClick={() => { fetchPermissions(); fetchBlocklist(); }}
            className="p-1.5 hover:bg-gray-100 dark:hover:bg-slate-700 rounded transition-colors"
            title="Refresh"
          >
            <RefreshCw className={`w-4 h-4 text-gray-500 ${loading ? 'animate-spin' : ''}`} />
          </button>
          <button
            onClick={() => setShowAddPermission(true)}
            className="flex items-center gap-1 px-2 py-1 text-xs bg-blue-600 hover:bg-blue-500 text-white rounded transition-colors"
          >
            <Plus className="w-3 h-3" />
            Add
          </button>
        </div>
      </div>

      {/* Add Permission Form */}
      {showAddPermission && (
        <div className="p-3 border border-blue-200 dark:border-blue-800 bg-blue-50 dark:bg-blue-900/20 rounded-lg space-y-2">
          <input
            type="text"
            placeholder="Permission name"
            value={newPermission.name}
            onChange={(e) => setNewPermission({ ...newPermission, name: e.target.value })}
            className="w-full px-2 py-1.5 text-sm border border-gray-300 dark:border-slate-600 rounded bg-white dark:bg-slate-800"
          />
          <input
            type="text"
            placeholder="Command prefix (e.g., 'docker ')"
            value={newPermission.command_prefix}
            onChange={(e) => setNewPermission({ ...newPermission, command_prefix: e.target.value })}
            className="w-full px-2 py-1.5 text-sm border border-gray-300 dark:border-slate-600 rounded bg-white dark:bg-slate-800 font-mono"
          />
          <input
            type="text"
            placeholder="Description (optional)"
            value={newPermission.description}
            onChange={(e) => setNewPermission({ ...newPermission, description: e.target.value })}
            className="w-full px-2 py-1.5 text-sm border border-gray-300 dark:border-slate-600 rounded bg-white dark:bg-slate-800"
          />
          <label className="flex items-center gap-2 text-sm text-gray-700 dark:text-slate-300">
            <input
              type="checkbox"
              checked={newPermission.requires_approval}
              onChange={(e) => setNewPermission({ ...newPermission, requires_approval: e.target.checked })}
              className="rounded"
            />
            Requires approval each time
          </label>
          <div className="flex justify-end gap-2">
            <button
              onClick={() => setShowAddPermission(false)}
              className="px-3 py-1 text-xs text-gray-600 dark:text-slate-400 hover:bg-gray-100 dark:hover:bg-slate-700 rounded"
            >
              Cancel
            </button>
            <button
              onClick={handleAddPermission}
              disabled={!newPermission.name || !newPermission.command_prefix}
              className="px-3 py-1 text-xs bg-blue-600 hover:bg-blue-500 disabled:bg-gray-400 text-white rounded"
            >
              Add Permission
            </button>
          </div>
        </div>
      )}

      {/* Permissions List */}
      <div className="space-y-2 max-h-[400px] overflow-y-auto">
        {permissions.map((perm) => (
          <div
            key={perm.id}
            className={`p-3 rounded-lg border ${
              perm.enabled
                ? 'border-gray-200 dark:border-slate-700 bg-white dark:bg-slate-800'
                : 'border-gray-100 dark:border-slate-800 bg-gray-50 dark:bg-slate-850 opacity-60'
            }`}
          >
            <div className="flex items-start justify-between">
              <div className="flex-1 min-w-0">
                <div className="font-medium text-sm text-gray-800 dark:text-slate-200">
                  {perm.name}
                </div>
                {perm.description && (
                  <div className="text-xs text-gray-500 dark:text-slate-400 mt-0.5">
                    {perm.description}
                  </div>
                )}
                <div className="text-xs font-mono text-gray-400 dark:text-slate-500 mt-1">
                  {perm.command_prefix && `prefix: ${perm.command_prefix}`}
                  {perm.command_pattern && `pattern: ${perm.command_pattern}`}
                  {perm.command_exact && `exact: ${perm.command_exact}`}
                </div>
                <div className="flex items-center gap-2 mt-1">
                  {perm.requires_approval ? (
                    <span className="text-xs px-1.5 py-0.5 bg-yellow-100 dark:bg-yellow-900/30 text-yellow-700 dark:text-yellow-400 rounded">
                      requires approval
                    </span>
                  ) : (
                    <span className="text-xs px-1.5 py-0.5 bg-green-100 dark:bg-green-900/30 text-green-700 dark:text-green-400 rounded">
                      auto-approve
                    </span>
                  )}
                  {perm.use_count > 0 && (
                    <span className="text-xs text-gray-400">
                      used {perm.use_count}x
                    </span>
                  )}
                </div>
              </div>
              <div className="flex items-center gap-1 ml-2">
                <button
                  onClick={() => togglePermission(perm.id, !perm.enabled)}
                  className="p-1.5 hover:bg-gray-100 dark:hover:bg-slate-700 rounded"
                  title={perm.enabled ? 'Disable' : 'Enable'}
                >
                  {perm.enabled ? (
                    <ToggleRight className="w-5 h-5 text-green-500" />
                  ) : (
                    <ToggleLeft className="w-5 h-5 text-gray-400" />
                  )}
                </button>
                <button
                  onClick={() => removePermission(perm.id)}
                  className="p-1.5 hover:bg-red-100 dark:hover:bg-red-900/30 rounded text-gray-400 hover:text-red-500"
                  title="Delete"
                >
                  <Trash2 className="w-4 h-4" />
                </button>
              </div>
            </div>
          </div>
        ))}
      </div>
    </div>
  );

  const renderBlocklistTab = () => (
    <div className="p-4 space-y-4">
      <div className="flex items-center justify-between">
        <h3 className="font-medium text-sm text-gray-800 dark:text-slate-200">Blocked Commands</h3>
        <button
          onClick={fetchBlocklist}
          className="p-1.5 hover:bg-gray-100 dark:hover:bg-slate-700 rounded transition-colors"
          title="Refresh"
        >
          <RefreshCw className={`w-4 h-4 text-gray-500 ${loading ? 'animate-spin' : ''}`} />
        </button>
      </div>

      <p className="text-xs text-gray-500 dark:text-slate-400">
        These commands are always blocked and cannot be executed, even with approval.
      </p>

      <div className="space-y-2 max-h-[400px] overflow-y-auto">
        {blocklist.map((entry) => (
          <div
            key={entry.id}
            className={`p-3 rounded-lg border ${
              entry.enabled
                ? 'border-red-200 dark:border-red-900 bg-red-50 dark:bg-red-900/20'
                : 'border-gray-100 dark:border-slate-800 bg-gray-50 dark:bg-slate-850 opacity-60'
            }`}
          >
            <div className="flex items-start justify-between">
              <div className="flex-1 min-w-0">
                <div className="font-medium text-sm text-gray-800 dark:text-slate-200">
                  {entry.name}
                </div>
                {entry.description && (
                  <div className="text-xs text-gray-500 dark:text-slate-400 mt-0.5">
                    {entry.description}
                  </div>
                )}
                <div className="text-xs font-mono text-gray-400 dark:text-slate-500 mt-1">
                  {entry.pattern_prefix && `prefix: ${entry.pattern_prefix}`}
                  {entry.pattern_regex && `regex: ${entry.pattern_regex}`}
                  {entry.pattern_exact && `exact: ${entry.pattern_exact}`}
                </div>
                <span className={`inline-block mt-1 text-xs px-1.5 py-0.5 rounded ${
                  entry.severity === 'critical' ? 'bg-red-200 dark:bg-red-800 text-red-800 dark:text-red-200' :
                  entry.severity === 'high' ? 'bg-orange-200 dark:bg-orange-800 text-orange-800 dark:text-orange-200' :
                  'bg-yellow-200 dark:bg-yellow-800 text-yellow-800 dark:text-yellow-200'
                }`}>
                  {entry.severity}
                </span>
              </div>
            </div>
          </div>
        ))}
      </div>
    </div>
  );

  return (
    <div className="h-full flex flex-col">
      {/* Sub-tabs */}
      <div className="flex-shrink-0 flex border-b border-gray-200 dark:border-slate-700">
        <button
          onClick={() => setSubTab('access')}
          className={`flex-1 px-3 py-2 text-xs font-medium transition-colors ${
            subTab === 'access'
              ? 'text-blue-600 dark:text-blue-400 border-b-2 border-blue-500'
              : 'text-gray-500 dark:text-slate-400 hover:text-gray-700'
          }`}
        >
          Access
        </button>
        <button
          onClick={() => setSubTab('permissions')}
          className={`flex-1 px-3 py-2 text-xs font-medium transition-colors ${
            subTab === 'permissions'
              ? 'text-blue-600 dark:text-blue-400 border-b-2 border-blue-500'
              : 'text-gray-500 dark:text-slate-400 hover:text-gray-700'
          }`}
        >
          Sudo Rules
        </button>
        <button
          onClick={() => setSubTab('blocklist')}
          className={`flex-1 px-3 py-2 text-xs font-medium transition-colors ${
            subTab === 'blocklist'
              ? 'text-blue-600 dark:text-blue-400 border-b-2 border-blue-500'
              : 'text-gray-500 dark:text-slate-400 hover:text-gray-700'
          }`}
        >
          Blocklist
        </button>
      </div>

      {/* Content */}
      <div className="flex-1 overflow-y-auto">
        {subTab === 'access' && renderAccessTab()}
        {subTab === 'permissions' && renderPermissionsTab()}
        {subTab === 'blocklist' && renderBlocklistTab()}
      </div>
    </div>
  );
}
