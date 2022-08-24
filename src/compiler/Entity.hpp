#pragma once

#include <utils/Types.hpp>
#include "Type.hpp"

namespace gdml {
    struct Entity {
    protected:
        std::shared_ptr<Namespace> m_namespace;
        std::string m_name;
        EntityType m_type;

        virtual void applyTypeDefinition() {}

        friend struct Namespace;

    public:
        Entity(
            std::shared_ptr<Namespace> container,
            std::string const& name,
            EntityType type
        );

        EntityType getType() const;
        std::string getFullName() const;
        std::string getName() const;
        bool hasParentNamespace() const;

        virtual bool isValue() const {
            return false;
        }
        virtual bool isType() const {
            return false;
        }
        virtual QualifiedType getValueType() const {
            return QualifiedType::NO_TYPE;
        }

        virtual ~Entity() = default;
    };

    struct ValueEntity : public Entity {
        virtual std::shared_ptr<Value> eval(Instance& instance) = 0;

        bool isValue() const override {
            return true;
        }
        ValueEntity(
            std::shared_ptr<Namespace> container, 
            std::string const& name,
            EntityType type
        );
    };

    struct TypeEntity : public Entity {
        std::shared_ptr<Type> type;

        bool isType() const override {
            return true;
        }
        QualifiedType getValueType() const override {
            return QualifiedType(type);
        }

        TypeEntity(
            std::shared_ptr<Namespace> container,
            std::string const& name,
            std::shared_ptr<Type> type
        );
    };

    struct Variable : public ValueEntity {
        QualifiedType type;
        std::shared_ptr<Value> value = nullptr;
        ast::VariableDeclExpr* declaration = nullptr;

        QualifiedType getValueType() const override {
            return type;
        }
        std::shared_ptr<Value> eval(Instance&) override {
            return value;
        }

        Variable(
            std::shared_ptr<Namespace> container,
            std::string const& name,
            QualifiedType const& type,
            std::shared_ptr<Value> value,
            ast::VariableDeclExpr* decl
        );
    };

    struct FunctionEntity : public ValueEntity {
        QualifiedFunType type;
        ast::AFunctionDeclStmt* declaration = nullptr;

        QualifiedType getValueType() const override {
            return type.into<Type>();
        }
        std::shared_ptr<Value> eval(Instance& instance) override;

        FunctionEntity(
            std::shared_ptr<Namespace> container,
            std::string const& name,
            QualifiedFunType const& type,
            ast::AFunctionDeclStmt* decl
        );
    };

    struct Namespace :
        public Entity,
        public std::enable_shared_from_this<Namespace>
    {
    protected:
        bool m_isGlobal;
        std::unordered_map<std::string, std::vector<std::shared_ptr<Entity>>> m_entities;

        void pushEntity(std::string const& name, std::shared_ptr<Entity> entity);
        
        std::shared_ptr<Entity> getEntity(
            std::string const& name,
            Option<EntityType> const& type,
            Option<std::vector<Parameter>> const& parameters
        ) const;
        std::shared_ptr<Namespace> getNamespaceMut(std::string const& name);
        std::shared_ptr<const Namespace> getNamespace(std::string const& name) const;
        std::shared_ptr<const Namespace> getNamespace(NamespaceParts const& name) const;

    public:
        Namespace(
            std::shared_ptr<Namespace> container,
            std::string const& name,
            bool isGlobal = false
        );

        bool isGlobal() const;

        bool hasEntity(
            std::string const& name,
            NamespaceParts const& currentNamespace,
            std::vector<NamespaceParts> const& testNamespaces,
            Option<EntityType> const& type,
            Option<std::vector<Parameter>> const& parameters
        ) const;

        std::shared_ptr<Entity> getEntity(
            std::string const& name,
            NamespaceParts const& currentNamespace,
            std::vector<NamespaceParts> const& testNamespaces,
            Option<EntityType> const& type,
            Option<std::vector<Parameter>> const& parameters
        ) const;

        template<class T, class... Args>
        std::shared_ptr<T> makeEntity(
            std::string const& name, Args&&... args
        ) {
            auto entity = std::make_shared<T>(
                shared_from_this(),
                name, std::forward<Args>(args)...
            );
            // apply type definition to entities that 
            // need to do that (like classes)
            // it can't be done in the constructor 
            // because shared_from_this isn't valid yet
            entity->applyTypeDefinition();
            pushEntity(name, entity);
            return entity;
        }
    };

    struct Class : public Namespace
    {
    protected:
        std::shared_ptr<ClassType> m_classType;

        void applyTypeDefinition() override;

        friend struct Namespace;

    public:
        Class(
            std::shared_ptr<Namespace> container,
            std::string const& name,
            std::shared_ptr<ClassType> classType
        );

        bool isType() const override {
            return true;
        }
        QualifiedType getValueType() const override {
            return QualifiedType(m_classType);
        }

        std::shared_ptr<ClassType> getClassType() const;
        std::shared_ptr<PointerType> getClassTypePointer() const;

        bool hasMember(std::string const& name) const;
        std::shared_ptr<Variable> getMember(std::string const& name) const;

        bool hasMemberFunction(
            std::string const& name,
            Option<std::vector<Parameter>> const& parameters
        ) const;
        std::shared_ptr<FunctionEntity> getMemberFunction(
            std::string const& name,
            Option<std::vector<Parameter>> const& parameters
        ) const;

        template<class T, class... Args>
        std::shared_ptr<T> makeMember(
            std::string const& name, Args&&... args
        ) {
            auto entity = std::make_shared<T>(
                shared_from_this(),
                name, std::forward<Args>(args)...
            );
            pushEntity(name, entity);
            return entity;
        }
    };
}
