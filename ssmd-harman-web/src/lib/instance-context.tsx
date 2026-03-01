"use client";

import {
  createContext,
  useContext,
  useState,
  useEffect,
  type ReactNode,
} from "react";
import { setApiInstance } from "./api";

export interface Instance {
  id: string;
  exchange: string;
  environment: string;
  version: string;
  healthy: boolean;
}

interface InstanceContextValue {
  instance: string | null;
  setInstance: (id: string) => void;
  instances: Instance[];
  loading: boolean;
}

const InstanceContext = createContext<InstanceContextValue>({
  instance: null,
  setInstance: () => {},
  instances: [],
  loading: true,
});

export function InstanceProvider({ children }: { children: ReactNode }) {
  const [instance, setInstanceState] = useState<string | null>(null);
  const [instances, setInstances] = useState<Instance[]>([]);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    fetch("/api/instances")
      .then((r) => r.json())
      .then((data) => {
        setInstances(data.instances || []);
        // Restore from sessionStorage if available and valid
        const saved = sessionStorage.getItem("harman-instance");
        if (saved && data.instances?.some((i: Instance) => i.id === saved)) {
          setInstanceState(saved);
          setApiInstance(saved);
        }
        setLoading(false);
      })
      .catch(() => setLoading(false));
  }, []);

  const setInstance = (id: string) => {
    setApiInstance(id);
    setInstanceState(id);
    sessionStorage.setItem("harman-instance", id);
  };

  return (
    <InstanceContext.Provider
      value={{ instance, setInstance, instances, loading }}
    >
      {children}
    </InstanceContext.Provider>
  );
}

export function useInstance() {
  return useContext(InstanceContext);
}
